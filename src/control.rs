// SPDX-License-Identifier: Apache-2.0

//! [`Control`]: a handle to `/dev/loop-control`, the loop-device factory.

use std::fs::File;
use std::io::{ErrorKind, Result};
use std::os::raw::{c_int, c_uint};

use crate::uapi::{LOOP_CTL_ADD, LOOP_CTL_GET_FREE, LOOP_CTL_REMOVE};

/// A handle to `/dev/loop-control`: the factory for `/dev/loopN` nodes.
///
/// It creates and removes nodes but never binds them; open a node with
/// [`Device::open`](crate::Device::open) to do that.
#[derive(Debug)]
pub struct Control(File);

impl Control {
    /// Opens `/dev/loop-control`. Needs `CAP_SYS_ADMIN`.
    ///
    /// The node is opened read-only: the control ioctls take their argument
    /// by value and the kernel applies no open-mode check to them.
    pub fn open() -> Result<Self> {
        Ok(Control(File::open("/dev/loop-control")?))
    }

    /// `LOOP_CTL_ADD` — create `/dev/loop{number}` and return its number.
    ///
    /// Fails with `EEXIST` if that number is already allocated.
    pub fn add(&self, number: c_uint) -> Result<c_uint> {
        let n = c_int::try_from(number).map_err(|_| ErrorKind::InvalidInput)?;
        LOOP_CTL_ADD.ioctl(&self.0, n)
    }

    /// `LOOP_CTL_REMOVE` — remove `/dev/loop{number}`.
    ///
    /// Fails with `EBUSY` if the node is still open or bound, and `ENODEV` if
    /// it doesn't exist.
    pub fn remove(&self, number: c_uint) -> Result<()> {
        let n = c_int::try_from(number).map_err(|_| ErrorKind::InvalidInput)?;
        LOOP_CTL_REMOVE.ioctl(&self.0, n)?;
        Ok(())
    }

    /// `LOOP_CTL_GET_FREE` — return the number of an unbound device, adding
    /// one if the pool has none free.
    ///
    /// The device is not reserved, so a concurrent caller can bind it first
    /// and leave [`Device::configure`](crate::Device::configure) failing with
    /// `EBUSY`; retry from here if that matters.
    pub fn get_free(&self) -> Result<c_uint> {
        LOOP_CTL_GET_FREE.ioctl(&self.0)
    }
}
