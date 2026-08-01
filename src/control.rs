// SPDX-License-Identifier: Apache-2.0

//! [`Control`]: the loop-control fd (`/dev/loop-control`). A factory for
//! [`LoopDevice`]s.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsFd;

use crate::Error;
use crate::config::Config;
use crate::device::LoopDevice;
use std::os::raw::c_int;

use crate::uapi::{LOOP_CTL_ADD, LOOP_CTL_GET_FREE, LOOP_CTL_REMOVE};

/// The loop-control fd (`/dev/loop-control`). A factory for [`LoopDevice`]s:
/// `add`, `remove`, `get_free`, and the `attach` convenience that allocates
/// a free device and configures it in one step.
#[derive(Debug)]
pub struct Control(File);

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
        Ok(Self(file))
    }

    /// Open `/dev/loop{number}` and wrap it in a [`LoopDevice`].
    fn open_device(number: u32) -> Result<LoopDevice, Error> {
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
        Ok(LoopDevice::new(number, file))
    }

    /// `LOOP_CTL_ADD` — create `/dev/loop{number}` and return a handle to
    /// it.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the request (e.g. the
    /// number is already in use — `EEXIST`).
    pub fn add(&self, number: u32) -> Result<LoopDevice, Error> {
        // Loop numbers are small and always fit in a positive c_int.
        #[allow(clippy::cast_possible_wrap)]
        LOOP_CTL_ADD
            .ioctl(self.0.as_fd(), number as c_int)
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_CTL_ADD",
                source,
            })?;
        Self::open_device(number)
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

    /// `LOOP_CTL_GET_FREE` — allocate (or reuse) a free loop device and
    /// return a handle to it.
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
        Self::open_device(number)
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
    /// `source.kind()` is [`std::io::ErrorKind::ResourceBusy`] (`EBUSY`). The
    /// crate deliberately bakes in no retry policy; a caller that wants to
    /// tolerate the race can retry `attach` on exactly that condition:
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
    /// # Errors
    ///
    /// [`Error::Usage`] if `config` holds a value the kernel would reject
    /// (see [`LoopDevice::configure`]). [`Error::LoopIoctl`] if the
    /// allocation fails, or if the configure fails — including the `EBUSY`
    /// race described above, which the caller may choose to retry.
    pub fn attach(&self, backing: &File, config: &Config) -> Result<LoopDevice, Error> {
        config.validate()?;
        let device = self.get_free()?;
        device.configure(backing, config)?;
        Ok(device)
    }
}
