// SPDX-License-Identifier: Apache-2.0

//! Kernel UAPI mirrors + ioctl-number declarations. This is the ONLY module
//! in the crate that needs `#![allow(unsafe_code)]` — every unsafe block
//! here is an iocuddle const constructor.
//!
//! The `#[repr(C)]` ioctl-argument structs live next to their safe wrappers
//! in [`super::config`] ([`super::config::LoopConfig`]) and
//! [`super::device`] ([`super::device::LoopInfo`]); this module only holds
//! the shared constants and the [`Ioctl::classic`] command declarations that
//! reference them. All field layouts and command numbers below mirror
//! `<linux/loop.h>`.
//!
//! Unlike device-mapper's `_IOWR(0xfd, N, struct)` ioctls, the loop ioctl
//! request numbers are bare `_IO(0x4C, n)` constants with no size field
//! baked in, so they can't be built with iocuddle's `Group::write_read`
//! (which would encode a struct size and produce the wrong request number).
//! Every one is therefore declared with [`Ioctl::classic`] against the exact
//! literal request number: struct-pointer ioctls as `Write<&T>`/`WriteRead<&T>`,
//! scalar-argument ioctls as `Write<c_int>` (the arg passed by value), and
//! no-argument ioctls as `Write<c_void>`.

#![allow(unsafe_code)]

use std::os::raw::{c_int, c_void};

use iocuddle::{Ioctl, Write, WriteRead};

pub(crate) const LO_NAME_SIZE: usize = 64;
pub(crate) const LO_KEY_SIZE: usize = 32;

/// `LOOP_MAJOR` — the block-device major number the loop driver owns. Fixed
/// at 7 in Linux; a `/dev/loopN` node is `(7, N)`.
pub(crate) const LOOP_MAJOR: u32 = 7;

/// `LO_FLAGS_READ_ONLY` — the loop device is read-only.
pub(crate) const LO_FLAGS_READ_ONLY: u32 = 1;

/// `LO_FLAGS_AUTOCLEAR` — the device auto-detaches when its last user
/// closes it.
pub(crate) const LO_FLAGS_AUTOCLEAR: u32 = 4;

/// `LO_FLAGS_PARTSCAN` — the kernel scans the backing file for a partition
/// table and creates partition devices.
pub(crate) const LO_FLAGS_PARTSCAN: u32 = 8;

/// `LO_FLAGS_DIRECT_IO` — I/O to the backing file bypasses the page cache.
pub(crate) const LO_FLAGS_DIRECT_IO: u32 = 16;

// SAFETY: `LOOP_CONFIGURE = 0x4C0A` is `_IO(0x4C, 0x0A)` per
// `<linux/loop.h>` — confirmed directly against the installed header, not
// from memory. The kernel reads a `struct loop_config` through the arg
// pointer, so the direction is `Write` and the type is
// `&super::config::LoopConfig`, a `#[repr(C)]` struct with private fields and
// an invariant-enforcing constructor, satisfying iocuddle's "T provides safe
// wrappers around its raw contents" contract. The request has no size field,
// so `Ioctl::classic` is used rather than `Group::write` (which would encode
// `size_of::<LoopConfig>`).
pub(crate) const LOOP_CONFIGURE: Ioctl<Write, &super::config::LoopConfig> =
    unsafe { Ioctl::classic(0x4C0A) };

// SAFETY: `LOOP_SET_STATUS64 = 0x4C04` is `_IO(0x4C, 0x04)`. The kernel
// reads a `LoopInfo` (`struct loop_info64`) through the arg pointer
// (direction `Write`).
// See `LOOP_CONFIGURE` for why `Ioctl::classic` is used.
// Declared for completeness of the loop UAPI surface; the public API mutates
// a bound device via the dedicated scalar ioctls rather than re-`SET_STATUS`.
#[allow(dead_code)]
pub(crate) const LOOP_SET_STATUS64: Ioctl<Write, &super::device::LoopInfo> =
    unsafe { Ioctl::classic(0x4C04) };

// SAFETY: `LOOP_GET_STATUS64 = 0x4C05` is `_IO(0x4C, 0x05)`. The kernel
// fills a `LoopInfo` (`struct loop_info64`) through the arg pointer, so the
// direction is `WriteRead` and the arg is passed as `&mut`. Every bit pattern
// the kernel can leave behind is a legal value of `LoopInfo` (all
// integer/byte-array fields). See `LOOP_CONFIGURE` for why `Ioctl::classic`
// is used.
pub(crate) const LOOP_GET_STATUS64: Ioctl<WriteRead, &super::device::LoopInfo> =
    unsafe { Ioctl::classic(0x4C05) };

// Scalar / no-arg loop ioctls, `_IO(0x4C, n)`. iocuddle's `Write<c_int>`
// passes the argument by value and `Write<c_void>` passes none; both return
// the kernel's non-negative result (a loop number for `CTL_ADD`/`GET_FREE`).
// SAFETY (all below): each request is the exact `_IO(0x4C, n)` number from
// `<linux/loop.h>`, and each kernel handler takes its argument by value (an
// fd, a size, a boolean, or a loop number) — never as a pointer — matching
// the `c_int`/`c_void` argument type.

/// `LOOP_SET_FD` — bind a backing file (arg = its raw fd).
// Declared for completeness of the loop UAPI surface; the public API binds a
// backing file via `LOOP_CONFIGURE` (`LOOP_SET_FD` is the older, split path).
#[allow(dead_code)]
pub(crate) const LOOP_SET_FD: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C00) };

/// `LOOP_CLR_FD` — detach the backing file (no argument).
pub(crate) const LOOP_CLR_FD: Ioctl<Write, c_void> = unsafe { Ioctl::classic(0x4C01) };

/// `LOOP_CHANGE_FD` — swap the backing file (arg = new raw fd).
pub(crate) const LOOP_CHANGE_FD: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C06) };

/// `LOOP_SET_CAPACITY` — re-read the backing file's size (no argument).
pub(crate) const LOOP_SET_CAPACITY: Ioctl<Write, c_void> = unsafe { Ioctl::classic(0x4C07) };

/// `LOOP_SET_DIRECT_IO` — toggle direct I/O (arg = 0/1).
pub(crate) const LOOP_SET_DIRECT_IO: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C08) };

/// `LOOP_SET_BLOCK_SIZE` — set the logical block size (arg = block size).
pub(crate) const LOOP_SET_BLOCK_SIZE: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C09) };

/// `LOOP_CTL_ADD` — create `/dev/loopN` (arg = desired number; returns it).
pub(crate) const LOOP_CTL_ADD: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C80) };

/// `LOOP_CTL_REMOVE` — remove `/dev/loopN` (arg = number).
pub(crate) const LOOP_CTL_REMOVE: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C81) };

/// `LOOP_CTL_GET_FREE` — allocate/return a free loop number (no argument).
pub(crate) const LOOP_CTL_GET_FREE: Ioctl<Write, c_void> = unsafe { Ioctl::classic(0x4C82) };
