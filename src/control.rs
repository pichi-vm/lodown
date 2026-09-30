// SPDX-License-Identifier: Apache-2.0

//! The `/dev/loop-control` handle.

use std::fs::File;
use std::io::{ErrorKind, Result};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, IntoRawFd, OwnedFd, RawFd};
use std::os::raw::{c_int, c_uint};

use crate::device::Device;
use crate::info::Configurable;
use crate::uapi::{LOOP_CTL_ADD, LOOP_CTL_GET_FREE, LOOP_CTL_REMOVE};

/// An open `/dev/loop-control`.
///
/// Needed only to obtain or destroy a loop device. Operating on one you
/// already hold does not go through the control node; see
/// [`Device`](crate::Device).
///
/// ```no_run
/// # fn main() -> std::io::Result<()> {
/// let control = lodown::Control::open()?;
/// let backing = std::fs::File::open("disk.img")?;
/// let device = control.attach(&backing, 0, lodown::Configurable::default())?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Control(File);

impl Control {
    /// Opens `/dev/loop-control`, which needs `CAP_SYS_ADMIN`.
    pub fn open() -> Result<Self> {
        Ok(Control(File::open("/dev/loop-control")?))
    }

    /// Runs `attach` with an exclusive BSD lock held on the control node.
    ///
    /// Purely an optimization, and only against other lockers: the kernel
    /// enforces nothing here. It serializes the get-free/configure pair so
    /// that concurrent claimants queue instead of colliding, which is what
    /// keeps the retry loop from repeating heavy work. systemd takes the
    /// same lock for the same reason, so we cooperate with it too.
    fn locked<T>(&self, attempt: impl FnOnce() -> Result<T>) -> Result<T> {
        // An advisory lock is a hint, not a guarantee. If the filesystem
        // backing /dev refuses it there is nothing to recover from — the
        // attempt below is still correct, just more likely to collide.
        let Ok(()) = self.0.lock() else {
            return attempt();
        };
        let result = attempt();
        let _ = self.0.unlock();
        result
    }

    /// Creates `/dev/loop{number}` (`LOOP_CTL_ADD`).
    ///
    /// Creates the node only. To attach a file to a new loop device, consider
    /// [`attach`](Self::attach) instead.
    pub fn add(&self, number: c_uint) -> Result<c_uint> {
        let n = c_int::try_from(number).map_err(|_| ErrorKind::InvalidInput)?;
        LOOP_CTL_ADD.ioctl(&self.0, n)
    }

    /// Removes `/dev/loop{number}` (`LOOP_CTL_REMOVE`).
    pub fn remove(&self, number: c_uint) -> Result<()> {
        let n = c_int::try_from(number).map_err(|_| ErrorKind::InvalidInput)?;
        LOOP_CTL_REMOVE.ioctl(&self.0, n)?;
        Ok(())
    }

    /// Returns an unbound loop number (`LOOP_CTL_GET_FREE`).
    ///
    /// To attach a file to a new loop device, consider [`attach`](Self::attach)
    /// instead.
    ///
    /// The number is not reserved: call this twice without binding and it
    /// answers the same number twice, and another caller may bind it before
    /// you do.
    pub fn get_free(&self) -> Result<c_uint> {
        LOOP_CTL_GET_FREE.ioctl(&self.0)
    }

    /// Attaches `backing` to a free loop device and returns it.
    ///
    /// The kernel offers no race-free way to do this. Asking for a free
    /// number and binding it are separate operations, and nothing reserves
    /// the number in between — so two callers can be handed the same one,
    /// and the loser's bind fails. This method wraps
    /// [`get_free`](Self::get_free), [`Device::open`](crate::Device::open)
    /// and [`Device::configure`](crate::Device::configure), retrying the
    /// sequence until it wins a device, which is the closest the interface
    /// allows.
    ///
    /// Each attempt holds an exclusive lock on the control node. systemd
    /// takes the same lock for the same purpose, so callers of either will
    /// queue rather than collide. The lock is advisory: it helps only
    /// against other programs that take it, and its absence costs extra
    /// retries rather than correctness.
    ///
    /// A `block_size` of zero keeps the kernel default.
    ///
    /// # Errors
    ///
    /// Anything that is not losing the race: an unsupported `block_size` or
    /// a closed `backing`, for instance, or no free number to bind at all.
    ///
    /// ```no_run
    /// use lodown::{Configurable, Control};
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let control = Control::open()?;
    /// let backing = std::fs::File::open("disk.img")?;
    ///
    /// let device = control.attach(&backing, 0, Configurable {
    ///     read_only: true,
    ///     ..Default::default()
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn attach(
        &self,
        backing: impl AsFd,
        block_size: c_uint,
        config: impl Into<Configurable>,
    ) -> Result<Device> {
        let config = config.into();
        let backing = backing.as_fd();

        loop {
            let claimed = self.locked(|| {
                let number = self.get_free()?;

                let device = match Device::open(number) {
                    Ok(device) => device,
                    // Removed, or still in rundown, since `get_free` spoke.
                    Err(e) if lost_the_race(&e) => return Ok(None),
                    Err(other) => return Err(other),
                };

                match device.configure(backing, block_size, config) {
                    Ok(()) => Ok(Some(device)),
                    // Another claimant bound this number first.
                    Err(e) if e.kind() == ErrorKind::ResourceBusy => Ok(None),
                    Err(other) => Err(other),
                }
            })?;

            if let Some(device) = claimed {
                return Ok(device);
            }
        }
    }
}

/// Borrows the underlying file.
///
/// [`attach`](Control::attach) takes [`File::lock`] here, so holding that
/// lock yourself serializes a sequence of operations against other callers
/// that take it too.
impl AsRef<File> for Control {
    fn as_ref(&self) -> &File {
        &self.0
    }
}

/// Provides the concrete mutable file reference required by some APIs.
impl AsMut<File> for Control {
    fn as_mut(&mut self) -> &mut File {
        &mut self.0
    }
}

impl AsFd for Control {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for Control {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl IntoRawFd for Control {
    fn into_raw_fd(self) -> RawFd {
        self.0.into_raw_fd()
    }
}

/// Takes the file back out. There is no `FromRawFd`, whose method is
/// `unsafe fn`; convert through [`File`] or [`OwnedFd`] instead.
impl From<Control> for File {
    fn from(control: Control) -> Self {
        control.0
    }
}

// Infallible for the same reason as `Device`'s: the ioctls validate the
// descriptor themselves, so a `Control` over the wrong file is a clean
// runtime error rather than unsoundness.
impl From<File> for Control {
    fn from(file: File) -> Self {
        Control(file)
    }
}

impl From<Control> for OwnedFd {
    fn from(control: Control) -> Self {
        control.0.into()
    }
}

impl From<OwnedFd> for Control {
    fn from(fd: OwnedFd) -> Self {
        Control(File::from(fd))
    }
}

/// Did we lose the race for this number between `get_free` and the open?
///
/// The node is gone entirely (`ENOENT`) once a removal lands, and `lo_open`
/// rejects a device still in `Lo_rundown` or `Lo_deleting` with `ENXIO`.
fn lost_the_race(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(ENXIO) || error.kind() == ErrorKind::NotFound
}

/// What `lo_open` reports for a device mid-teardown.
const ENXIO: i32 = 6;

#[cfg(test)]
mod tests {
    use std::io::Error;

    use super::*;

    /// `attach` retries a lost race but not a caller's mistake.
    #[test]
    fn only_a_lost_race_is_retried() {
        const ENOENT: i32 = 2;
        const EINVAL: i32 = 22;

        assert!(lost_the_race(&Error::from_raw_os_error(ENXIO)));
        assert!(lost_the_race(&Error::from_raw_os_error(ENOENT)));

        assert!(!lost_the_race(&Error::from_raw_os_error(EINVAL)));
        assert!(!lost_the_race(&Error::from(ErrorKind::PermissionDenied)));
    }
}
