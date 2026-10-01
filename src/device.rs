// SPDX-License-Identifier: Apache-2.0

//! The `/dev/loopN` handle.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Result, Seek, SeekFrom, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, IntoRawFd, OwnedFd, RawFd};
use std::os::raw::{c_int, c_uint};

use zerocopy::FromZeros;

use crate::info::{Configurable, Readable, Writable};
use crate::uapi::*;

/// An open `/dev/loopN` node.
///
/// A `Device` may be bound to a backing file or not, and needs no
/// [`Control`](crate::Control) either way.
///
/// Reading and writing a `Device` reads and writes the backing file's
/// bytes, through the block layer.
///
/// Dropping a `Device` closes the handle but leaves the backing file
/// attached. Use [`clear`](Self::clear), or [`Writable::autoclear`] for
/// cleanup that survives a crash.
#[derive(Debug)]
pub struct Device(File);

impl Device {
    /// Opens an existing `/dev/loop{number}` read-write.
    ///
    /// Opens a loop device number you already know. To claim and bind a free
    /// device, consider [`Control::attach`](crate::Control::attach) instead.
    ///
    /// Opening the node read-only forces the loop device to be read-only, so
    /// this opens it read-write. Request a read-only binding with
    /// [`Configurable::read_only`].
    pub fn open(number: c_uint) -> Result<Self> {
        let path = format!("/dev/loop{number}");
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Device(file))
    }

    /// Binds `backing` and applies `config` in one ioctl (`LOOP_CONFIGURE`).
    ///
    /// To attach a file to a new loop device, consider
    /// [`Control::attach`](crate::Control::attach) instead: binding a device
    /// you selected yourself is a race this call does not handle.
    ///
    /// A `block_size` of zero keeps the kernel default. An `O_RDONLY`
    /// `backing` yields a read-only device whatever `config` asks for.
    ///
    /// # Errors
    ///
    /// `EBUSY` if another caller bound this device first.
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

    /// Asks the kernel to detach the backing file (`LOOP_CLR_FD`).
    ///
    /// Detaches immediately only if nothing else holds the device open.
    /// Otherwise it turns on [`Writable::autoclear`] and the kernel detaches
    /// at last close, so `Ok(())` means "detached, or scheduled to detach".
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

/// Borrows the underlying file.
///
/// [`try_clone`](File::try_clone) is safe against [`clear`](Device::clear):
/// a clone does not count as another user of the device, so the detach
/// still happens immediately. A separately opened handle defers it.
impl AsRef<File> for Device {
    fn as_ref(&self) -> &File {
        &self.0
    }
}

/// Provides the concrete mutable file reference required by some APIs.
impl AsMut<File> for Device {
    fn as_mut(&mut self) -> &mut File {
        &mut self.0
    }
}

impl AsFd for Device {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for Device {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl IntoRawFd for Device {
    fn into_raw_fd(self) -> RawFd {
        self.0.into_raw_fd()
    }
}

/// Takes the file back out. There is no `FromRawFd`, whose method is
/// `unsafe fn`; convert through [`File`] or [`OwnedFd`] instead.
impl From<Device> for File {
    fn from(device: Device) -> Self {
        device.0
    }
}

// Infallible because the ioctls validate the descriptor themselves: one
// issued against something that is not a loop device fails with `ENOTTY`,
// so wrapping the wrong file is a clean runtime error rather than
// unsoundness, and no `TryFrom` could check more than that.
impl From<File> for Device {
    fn from(file: File) -> Self {
        Device(file)
    }
}

impl From<Device> for OwnedFd {
    fn from(device: Device) -> Self {
        device.0.into()
    }
}

impl From<OwnedFd> for Device {
    fn from(fd: OwnedFd) -> Self {
        Device(File::from(fd))
    }
}

// Implemented for `&Device` as well as `Device`, mirroring `File`, so a
// shared handle can still do I/O. Both share one kernel file offset.

impl Read for Device {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.0.read(buf)
    }
}

/// [`flush`](Write::flush) does nothing, as on any [`File`]: it reaches the
/// page cache and no further. Call [`sync_all`](File::sync_all), or detach,
/// when the backing file must see the write.
impl Write for Device {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        self.0.flush()
    }
}

impl Seek for Device {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        self.0.seek(pos)
    }
}

impl Read for &Device {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (&self.0).read(buf)
    }
}

impl Write for &Device {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (&self.0).write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        (&self.0).flush()
    }
}

impl Seek for &Device {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        (&self.0).seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::Control;

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

    /// Binds a fresh 1 MiB backing file.
    fn bind(tag: &str, config: Configurable) -> Bound {
        let control =
            Control::open().expect("open /dev/loop-control; test requires root or CAP_SYS_ADMIN");

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
                return Bound { device, path };
            }
        }
        panic!("kept losing the get-free/configure race");
    }

    /// Why `read_only` and `direct_io` are absent from [`Writable`].
    ///
    /// `LOOP_SET_STATUS64` masks both off and still reports success. The
    /// public API cannot express that, so drive the ioctl with raw flags.
    #[test]
    #[ignore = "requires root or CAP_SYS_ADMIN"]
    fn set_status_silently_ignores_attempts_to_set_configure_only_flags() {
        let bound = bind("set-ro", Configurable::default());
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
    #[ignore = "requires root or CAP_SYS_ADMIN"]
    fn set_status_silently_ignores_attempts_to_clear_read_only() {
        let config = Configurable {
            read_only: true,
            ..Default::default()
        };
        let bound = bind("clear-ro", config);
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
