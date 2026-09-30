// SPDX-License-Identifier: Apache-2.0

//! The `/dev/loopN` handle.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Result};
use std::os::fd::{AsFd, AsRawFd};
use std::os::raw::{c_int, c_uint};

use zerocopy::FromZeros;

use crate::info::{Configurable, Readable, Writable};
use crate::uapi::*;

/// An opened `/dev/loopN` node.
///
/// Dropping this closes the node but leaves any backing file attached; use
/// [`clear`](Self::clear) or [`Writable::autoclear`] to detach.
#[derive(Debug)]
pub struct Device(File);

impl Device {
    /// Opens an existing `/dev/loop{number}` read-write.
    ///
    /// `LOOP_CONFIGURE` forces `LO_FLAGS_READ_ONLY` when the node was opened
    /// read-only, so read-write is the only mode that can yield a writable
    /// device; request a read-only one with [`Configurable::read_only`].
    pub fn open(number: c_uint) -> Result<Self> {
        let path = format!("/dev/loop{number}");
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Device(file))
    }

    /// Binds `backing` and applies `config` in one ioctl (`LOOP_CONFIGURE`).
    ///
    /// A `block_size` of zero keeps the kernel default. An `O_RDONLY`
    /// `backing` yields a read-only device whatever `config` asks for.
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

    /// Detaches the backing file, consuming the handle (`LOOP_CLR_FD`).
    pub fn clear(self) -> Result<()> {
        LOOP_CLR_FD.ioctl(&self.0)?;
        Ok(())
    }

    /// Swaps in a new backing file (`LOOP_CHANGE_FD`).
    ///
    /// Valid only on a read-only device backed by a file of the same size.
    /// [`Writable::file_name`] keeps naming the old file.
    pub fn change(&self, backing: impl AsFd) -> Result<()> {
        LOOP_CHANGE_FD.ioctl(&self.0, backing.as_fd().as_raw_fd())?;
        Ok(())
    }

    /// Re-reads the backing file's size (`LOOP_SET_CAPACITY`).
    pub fn set_capacity(&self) -> Result<()> {
        LOOP_SET_CAPACITY.ioctl(&self.0)?;
        Ok(())
    }

    /// Toggles direct I/O (`LOOP_SET_DIRECT_IO`).
    ///
    /// Reports failure, where [`configure`](Self::configure) would silently
    /// clear [`Configurable::direct_io`] instead.
    pub fn set_direct_io(&self, enable: bool) -> Result<()> {
        LOOP_SET_DIRECT_IO.ioctl(&self.0, c_int::from(enable))?;
        Ok(())
    }

    /// Sets the logical block size (`LOOP_SET_BLOCK_SIZE`).
    pub fn set_block_size(&self, block_size: c_uint) -> Result<()> {
        let n = c_int::try_from(block_size).map_err(|_| ErrorKind::InvalidInput)?;
        LOOP_SET_BLOCK_SIZE.ioctl(&self.0, n)?;
        Ok(())
    }

    /// Reads the current state (`LOOP_GET_STATUS64`).
    pub fn status(&self) -> Result<Readable> {
        let mut info = LoopInfo::new_zeroed();
        LOOP_GET_STATUS64.ioctl(&self.0, &mut info)?;
        Ok(info.into())
    }

    /// Changes the [`Writable`] state of a bound device (`LOOP_SET_STATUS64`).
    ///
    /// The kernel applies only the fields it accepts and reports success
    /// either way, so [`Writable::partscan`] can only be turned on here.
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

    /// Building the node path must not panic on the highest legal number.
    ///
    /// `/dev/loop1048575` is exactly 16 bytes, the length at which a fixed
    /// 16-byte buffer leaves no room for a NUL.
    #[test]
    fn open_of_an_absent_high_number_errors_rather_than_panicking() {
        assert!(Device::open(1_048_575).is_err());
        assert!(Device::open(u32::MAX).is_err());
    }

    /// A bound loop device and its backing file, detached on drop.
    ///
    /// Duplicated from the integration suite because these cases reach past
    /// the public API to the raw ioctl, which `tests/` cannot see.
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

    /// Why `read_only` and `direct_io` are absent from [`Writable`].
    ///
    /// `LOOP_SET_STATUS64` masks both off and still reports success. The
    /// public API cannot express that, so drive the ioctl with raw flags.
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

    /// The same in reverse: a configured `read_only` cannot be cleared.
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
