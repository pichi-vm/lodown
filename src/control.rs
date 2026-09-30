// SPDX-License-Identifier: Apache-2.0

//! The `/dev/loop-control` handle.

use std::fs::File;
use std::io::{ErrorKind, Result};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, IntoRawFd, OwnedFd, RawFd};
use std::os::raw::{c_int, c_uint};

use crate::uapi::{LOOP_CTL_ADD, LOOP_CTL_GET_FREE, LOOP_CTL_REMOVE};

/// Creates and removes `/dev/loopN` nodes.
///
/// Binding a node to a backing file is [`Device`](crate::Device)'s job.
#[derive(Debug)]
pub struct Control(File);

impl Control {
    /// Opens `/dev/loop-control`, which requires `CAP_SYS_ADMIN`.
    pub fn open() -> Result<Self> {
        Ok(Control(File::open("/dev/loop-control")?))
    }

    /// Creates `/dev/loop{number}` (`LOOP_CTL_ADD`).
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

    /// Claims an unbound loop number (`LOOP_CTL_GET_FREE`).
    ///
    /// The number is not reserved: a concurrent caller can bind it first,
    /// leaving [`Device::configure`](crate::Device::configure) to fail with
    /// `EBUSY`. Retry from here if that matters.
    pub fn get_free(&self) -> Result<c_uint> {
        LOOP_CTL_GET_FREE.ioctl(&self.0)
    }
}

/// Exposes [`File`]-specific operations such as [`File::try_clone`] while
/// preserving ownership of the control handle.
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
