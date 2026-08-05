// SPDX-License-Identifier: Apache-2.0

//! The `/dev/loop-control` handle.

use std::fs::File;
use std::io::{ErrorKind, Result};
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
