// SPDX-License-Identifier: Apache-2.0

//! [`Device`]: a handle to an opened `/dev/loopN`.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Result};
use std::os::fd::{AsFd, AsRawFd};
use std::os::raw::{c_int, c_uint};

use zerocopy::FromZeros;

use crate::info::{Configurable, Readable, Writable};
use crate::uapi::*;

/// A handle to an opened `/dev/loopN` node.
///
/// Dropping a `Device` closes the node; it does *not* detach the backing
/// file. Call [`clear`](Self::clear) to detach, or set
/// [`Writable::autoclear`] so the kernel detaches on last close.
#[derive(Debug)]
pub struct Device(File);

impl Device {
    /// Opens `/dev/loop{number}`, which must already exist (see
    /// [`Control::add`](crate::Control::add) and
    /// [`Control::get_free`](crate::Control::get_free)).
    ///
    /// The node is opened read-write. `LOOP_CONFIGURE` forces
    /// `LO_FLAGS_READ_ONLY` on a device whose node was opened read-only, so a
    /// read-write open is the only way [`configure`](Self::configure) can
    /// produce a writable device; ask for a read-only one with
    /// [`Configurable::read_only`] instead.
    pub fn open(number: c_uint) -> Result<Self> {
        let path = format!("/dev/loop{number}");
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Device(file))
    }

    /// `LOOP_CONFIGURE` — bind `backing` to this device and apply `config` in
    /// a single ioctl. A `block_size` of zero keeps the kernel's default.
    ///
    /// The device is writable only if `backing` was opened read-write; an
    /// `O_RDONLY` backing file yields a read-only device regardless of
    /// [`Configurable::read_only`].
    pub fn configure(
        &self,
        backing: impl AsFd,
        block_size: c_uint,
        config: impl Into<Configurable>,
    ) -> Result<()> {
        let info = LoopInfo::from(config.into());
        let config = info.config(backing.as_fd().as_raw_fd().cast_unsigned(), block_size);
        LOOP_CONFIGURE.ioctl(&self.0, &config)?;
        Ok(())
    }

    /// `LOOP_CLR_FD` — detach the backing file, consuming this handle.
    pub fn clear(self) -> Result<()> {
        LOOP_CLR_FD.ioctl(&self.0)?;
        Ok(())
    }

    /// `LOOP_CHANGE_FD` — swap in a new backing file.
    ///
    /// Only valid for a read-only device backed by a file of the same size.
    /// The kernel does not update the recorded
    /// [`file_name`](Writable::file_name).
    pub fn change_backing(&self, backing: impl AsFd) -> Result<()> {
        LOOP_CHANGE_FD.ioctl(&self.0, backing.as_fd().as_raw_fd())?;
        Ok(())
    }

    /// `LOOP_SET_CAPACITY` — re-read the backing file's current size.
    pub fn set_capacity(&self) -> Result<()> {
        LOOP_SET_CAPACITY.ioctl(&self.0)?;
        Ok(())
    }

    /// `LOOP_SET_DIRECT_IO` — enable or disable page-cache-bypassing I/O.
    ///
    /// Unlike [`configure`](Self::configure), which silently clears
    /// [`Configurable::direct_io`] when it can't be honoured, this reports
    /// the failure.
    pub fn set_direct_io(&self, enable: bool) -> Result<()> {
        LOOP_SET_DIRECT_IO.ioctl(&self.0, c_int::from(enable))?;
        Ok(())
    }

    /// `LOOP_SET_BLOCK_SIZE` — set the logical block size in bytes, which
    /// must be a power of two between 512 and the page size.
    pub fn set_block_size(&self, block_size: c_uint) -> Result<()> {
        let n = c_int::try_from(block_size).map_err(|_| ErrorKind::InvalidInput)?;
        LOOP_SET_BLOCK_SIZE.ioctl(&self.0, n)?;
        Ok(())
    }

    /// `LOOP_GET_STATUS64` — read this device's current state.
    ///
    /// Fails with `ENXIO` if no backing file is bound.
    pub fn status(&self) -> Result<Readable> {
        let mut info = LoopInfo::new_zeroed();
        LOOP_GET_STATUS64.ioctl(&self.0, &mut info)?;
        Ok(info.into())
    }

    /// `LOOP_SET_STATUS64` — change the [`Writable`] state of a bound device.
    ///
    /// The kernel masks this to the fields it accepts and reports success
    /// regardless, so [`Writable::partscan`] can only be turned on here. The
    /// two [`Configurable`]-only flags aren't expressible by construction.
    pub fn set_status(&self, status: impl Into<Writable>) -> Result<()> {
        let info = LoopInfo::from(status.into());
        LOOP_SET_STATUS64.ioctl(&self.0, &info)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Control;

    /// `/dev/loop1048575` — the highest legal loop number
    /// (`MINORMASK >> LOOP_PART_SHIFT`) and exactly 16 bytes, the length at
    /// which a fixed 16-byte path buffer leaves no room for a NUL. Building
    /// the path must not panic on it, whether or not the node exists.
    #[test]
    fn open_of_an_absent_high_number_errors_rather_than_panicking() {
        assert!(Device::open(1_048_575).is_err());
        assert!(Device::open(u32::MAX).is_err());
    }

    /// A bound loop device and its backing file, detached on drop.
    ///
    /// The integration suite has a nicer version of this, but these cases
    /// have to reach past the public API to the raw ioctl, and `tests/` can't
    /// see [`crate::uapi`].
    struct Bound {
        device: Device,
        path: PathBuf,
    }

    impl Drop for Bound {
        fn drop(&mut self) {
            let _ = LOOP_CLR_FD.ioctl(&self.device.0);
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Binds a fresh 1 MiB backing file, or `None` without `CAP_SYS_ADMIN`.
    fn bind(tag: &str, config: Configurable) -> Option<Bound> {
        // `LODOWN_REQUIRE_ROOT` turns this skip into a failure, so a CI job
        // that loses its privileges says so instead of passing vacuously.
        let control = match Control::open() {
            Ok(control) => control,
            Err(error) => {
                assert!(
                    std::env::var_os("LODOWN_REQUIRE_ROOT").is_none(),
                    "LODOWN_REQUIRE_ROOT is set, but /dev/loop-control could not be opened: {error}"
                );
                eprintln!("skip: requires root (or CAP_SYS_ADMIN) for /dev/loop-control");
                return None;
            }
        };

        let path =
            std::env::temp_dir().join(format!("lodown-unit-{tag}-{}.img", std::process::id()));
        let backing = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .expect("create backing file");
        backing.set_len(1 << 20).expect("size backing file");

        // `get_free` doesn't reserve, so retry the claim/configure pair.
        for _ in 0..100 {
            let number = control.get_free().expect("get_free");
            let device = Device::open(number).expect("open device node");
            if device.configure(&backing, 0, config).is_ok() {
                return Some(Bound { device, path });
            }
        }
        panic!("kept losing the get-free/configure race");
    }

    /// The reason `read_only` and `direct_io` live on [`Configurable`] rather
    /// than [`Writable`]: `LOOP_SET_STATUS64` masks both off and still reports
    /// success, so exposing them on the `set_status` input would be a lie.
    ///
    /// This can't be written against the public API — the split makes the
    /// input unrepresentable — so it drives the ioctl with raw flags instead.
    #[test]
    fn set_status_silently_ignores_attempts_to_set_configure_only_flags() {
        let Some(bound) = bind("set-ro", Configurable::default()) else {
            return;
        };
        assert!(!bound.device.status().expect("status").read_only);

        let mut info = LoopInfo::new_zeroed();
        info.flags = LO_FLAGS_READ_ONLY | LO_FLAGS_DIRECT_IO;
        LOOP_SET_STATUS64
            .ioctl(&bound.device.0, &info)
            .expect("the ioctl reports success either way");

        let status = bound.device.status().expect("status");
        assert!(
            !status.read_only,
            "LOOP_SET_STATUS64 must not be able to set read_only"
        );
        assert!(
            !status.direct_io,
            "LOOP_SET_STATUS64 must not be able to set direct_io"
        );
    }

    /// The same in the other direction: a configure-time `read_only` can't be
    /// taken back off with `LOOP_SET_STATUS64` either.
    #[test]
    fn set_status_silently_ignores_attempts_to_clear_read_only() {
        let config = Configurable {
            read_only: true,
            ..Default::default()
        };
        let Some(bound) = bind("clear-ro", config) else {
            return;
        };
        assert!(bound.device.status().expect("status").read_only);

        let info = LoopInfo::new_zeroed(); // every flag off, read_only included
        LOOP_SET_STATUS64
            .ioctl(&bound.device.0, &info)
            .expect("the ioctl reports success either way");

        assert!(
            bound.device.status().expect("status").read_only,
            "LOOP_SET_STATUS64 must not be able to clear read_only"
        );
    }
}
