// SPDX-License-Identifier: Apache-2.0

//! [`Control`]: the loop-control fd (`/dev/loop-control`). A factory for
//! [`LoopDevice`]s.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsFd;
use std::sync::Arc;

use crate::Error;
use crate::config::Config;
use crate::device::{Detached, LoopDevice, Removed};
use std::os::raw::c_int;

use crate::uapi::{LOOP_CTL_ADD, LOOP_CTL_GET_FREE, LOOP_CTL_REMOVE};

/// The loop-control fd (`/dev/loop-control`). A factory for [`LoopDevice`]s:
/// `add`, `remove`, `get_free`, `by_number`, and the `attach` convenience that
/// allocates a free device and configures it in one step. `add` returns a
/// [`Removed`] guard (drop removes the node it created) and `attach` returns a
/// [`Detached`] guard (drop detaches the binding it made); `get_free` and
/// `by_number` return a plain [`LoopDevice`], since they acquire nothing that
/// this crate owns.
#[derive(Debug)]
pub struct Control(Arc<File>);

impl Control {
    /// Open `/dev/loop-control`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the control node can't be opened (typically because
    /// the process lacks `CAP_SYS_ADMIN`, or the `loop` module isn't
    /// loaded).
    pub fn open() -> Result<Self, Error> {
        let file =
            OpenOptions::new().read(true).write(true).open("/dev/loop-control").map_err(
                |source| {
                    Error::Io(io::Error::new(
                        source.kind(),
                        format!(
                            "cannot open /dev/loop-control (need CAP_SYS_ADMIN and the loop module loaded): {source}"
                        ),
                    ))
                },
            )?;
        Ok(Self(Arc::new(file)))
    }

    /// Open `/dev/loop{number}` and wrap it in a [`LoopDevice`], sharing this
    /// control's `/dev/loop-control` handle so the device can remove itself.
    fn open_device(&self, number: u32) -> Result<LoopDevice, Error> {
        let path = format!("/dev/loop{number}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| {
                Error::Io(io::Error::new(
                    source.kind(),
                    format!("cannot open {path}: {source}"),
                ))
            })?;
        Ok(LoopDevice::new(number, file, Arc::clone(&self.0)))
    }

    /// Open an existing `/dev/loop{number}` and return a plain
    /// [`LoopDevice`] handle — no `LOOP_CTL_ADD`, and (unlike [`add`](Self::add)
    /// / [`attach`](Self::attach)) no auto-cleaning guard, since this handle
    /// did not create the device or its binding and so must not tear either
    /// down.
    ///
    /// This does not check whether the device is bound; a later operation
    /// surfaces the kernel's error (e.g. `ENXIO`) if it isn't.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if `/dev/loop{number}` can't be opened (e.g. it does
    /// not exist).
    pub fn by_number(&self, number: u32) -> Result<LoopDevice, Error> {
        self.open_device(number)
    }

    /// `LOOP_CTL_ADD` — create `/dev/loop{number}` and return a [`Removed`]
    /// guard whose drop removes the node it created.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the request (e.g. the
    /// number is already in use — `EEXIST`).
    pub fn add(&self, number: u32) -> Result<Removed, Error> {
        // Loop numbers are small and always fit in a positive c_int.
        #[allow(clippy::cast_possible_wrap)]
        LOOP_CTL_ADD
            .ioctl(self.0.as_fd(), number as c_int)
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_CTL_ADD",
                source,
            })?;
        // The node now exists; if opening it fails, best-effort remove it so
        // we don't leak a node we just created.
        match self.open_device(number) {
            Ok(device) => Ok(Removed::from(device)),
            Err(err) => {
                let _ = self.remove(number);
                Err(err)
            }
        }
    }

    /// `LOOP_CTL_REMOVE` — remove `/dev/loop{number}`.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the request (e.g. the
    /// device is still in use — `EBUSY`).
    pub fn remove(&self, number: u32) -> Result<(), Error> {
        // Loop numbers are small and always fit in a positive c_int.
        #[allow(clippy::cast_possible_wrap)]
        LOOP_CTL_REMOVE
            .ioctl(self.0.as_fd(), number as c_int)
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_CTL_REMOVE",
                source,
            })?;
        Ok(())
    }

    /// `LOOP_CTL_GET_FREE` — obtain a free loop device and return a plain
    /// [`LoopDevice`] handle.
    ///
    /// This returns no guard: `LOOP_CTL_GET_FREE` hands back an *existing*
    /// device from the kernel's pool (the same one the next caller would get)
    /// rather than allocating one this handle owns, so it must not be removed
    /// on drop. Use [`attach`](Self::attach) to bind a backing file and get a
    /// [`Detached`] guard for the binding, or [`add`](Self::add) to create and
    /// own a specific node.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel can't provide a free device.
    pub fn get_free(&self) -> Result<LoopDevice, Error> {
        // `LOOP_CTL_GET_FREE` takes no argument and returns the free loop
        // number as the (non-negative) ioctl result.
        let number =
            LOOP_CTL_GET_FREE
                .ioctl(self.0.as_fd())
                .map_err(|source| Error::LoopIoctl {
                    op: "LOOP_CTL_GET_FREE",
                    source,
                })?;
        self.open_device(number)
    }

    /// Allocate a free loop device via [`Control::get_free`] and bind
    /// `backing` to it with `config` in one step (`LOOP_CTL_GET_FREE`
    /// followed by `LOOP_CONFIGURE`).
    ///
    /// This is a single attempt: `LOOP_CTL_GET_FREE` does not *reserve* the
    /// number it returns, so between the allocation and the `LOOP_CONFIGURE`
    /// a concurrent process can claim the same device first. When that
    /// happens the configure fails with
    /// [`Error::LoopIoctl`]`{ op: "LOOP_CONFIGURE", .. }` whose
    /// `source.kind()` is [`std::io::ErrorKind::ResourceBusy`] (`EBUSY`). On
    /// that failure this method only closes its own handle to the node — it
    /// does *not* detach or remove it, so it never disturbs the device the
    /// winner just configured. The crate bakes in no retry policy; a caller
    /// that wants to tolerate the race can retry `attach` on exactly that
    /// condition:
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use std::io::ErrorKind;
    /// use lodown::{Config, Control, Error};
    ///
    /// # fn main() -> Result<(), Error> {
    /// let control = Control::open()?;
    /// let backing = File::open("/path/to/backing.img")?;
    /// let config = Config::new();
    ///
    /// let device = loop {
    ///     match control.attach(&backing, &config) {
    ///         Ok(device) => break device,
    ///         // Another process claimed the free number first; try again.
    ///         Err(Error::LoopIoctl { source, .. })
    ///             if source.kind() == ErrorKind::ResourceBusy => continue,
    ///         Err(other) => return Err(other),
    ///     }
    /// };
    /// # let _ = device;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The returned [`Detached`] guard detaches the backing file on drop
    /// (leaving the pool node in place, since `get_free` did not create it);
    /// unwrap it (`LoopDevice::from(..)`) to keep the binding past the scope.
    /// The guard is armed only after `configure` succeeds, so a failed attach
    /// leaves nothing to tear down.
    ///
    /// # Errors
    ///
    /// [`Error::Usage`] if `config` holds a value the kernel would reject
    /// (see [`LoopDevice::configure`]). [`Error::LoopIoctl`] if the
    /// allocation fails, or if the configure fails — including the `EBUSY`
    /// race described above, which the caller may choose to retry.
    pub fn attach(&self, backing: impl AsFd, config: &Config) -> Result<Detached, Error> {
        config.validate()?;
        // Hold a plain handle during the get-free/configure window: if the
        // configure loses the race (or fails for any reason), this handle just
        // closes its fd on drop — it never detaches or removes a device it may
        // not own. Arm the detach guard only once the binding is established.
        let device = self.get_free()?;
        device.configure(backing, config)?;
        Ok(Detached::from(device))
    }
}
