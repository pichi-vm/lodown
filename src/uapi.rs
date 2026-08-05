// SPDX-License-Identifier: Apache-2.0

//! Mirrors of `<linux/loop.h>`; the safe views live in [`super::info`].
//!
//! The only module needing `allow(unsafe_code)`: every unsafe block is an
//! iocuddle const constructor.
//!
//! The loop request numbers are bare `_IO(0x4C, n)` with no size field, so
//! iocuddle's `Group::*` builders would encode a struct size and produce the
//! wrong number. Each is declared with [`Ioctl::classic`] against the literal
//! instead: struct-pointer ioctls as `Write<&T>`/`WriteRead<&T>`, by-value
//! ioctls as `Write<c_int>`, and no-argument ioctls as `Write<c_void>`.

#![allow(unreachable_pub)]
#![allow(unsafe_code)]

use std::os::raw::{c_int, c_void};

use iocuddle::{Ioctl, Write, WriteRead};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const LO_NAME_SIZE: usize = 64;
pub const LO_KEY_SIZE: usize = 32;

/// The block-device major the loop driver owns.
#[allow(dead_code)]
pub const LOOP_MAJOR: u32 = 7;

/// Read-only device; `LOOP_CONFIGURE` only.
pub const LO_FLAGS_READ_ONLY: u32 = 1;

/// Detach on last close; the only flag `LOOP_SET_STATUS64` can clear.
pub const LO_FLAGS_AUTOCLEAR: u32 = 4;

/// Scan for partitions; `LOOP_SET_STATUS64` can set but not clear it.
pub const LO_FLAGS_PARTSCAN: u32 = 8;

/// Bypass the page cache; `LOOP_SET_STATUS64` silently masks it off.
pub const LO_FLAGS_DIRECT_IO: u32 = 16;

// SAFETY: `LOOP_CONFIGURE = 0x4C0A` is `_IO(0x4C, 0x0A)` per
// `<linux/loop.h>` — confirmed directly against the installed header, not
// from memory. The kernel reads a `struct loop_config` through the arg
// pointer, so the direction is `Write` and the type is `&LoopConfig`, a
// `#[repr(C)]` struct with private fields and an invariant-enforcing
// constructor, satisfying iocuddle's "T provides safe wrappers around its raw
// contents" contract. The request has no size field, so `Ioctl::classic` is
// used rather than `Group::write` (which would encode `size_of::<LoopConfig>`).
pub const LOOP_CONFIGURE: Ioctl<Write, &LoopConfig> = unsafe { Ioctl::classic(0x4C0A) };

// SAFETY: `LOOP_SET_STATUS64 = 0x4C04` is `_IO(0x4C, 0x04)`. The kernel
// reads a `LoopInfo` (`struct loop_info64`) through the arg pointer
// (direction `Write`). See `LOOP_CONFIGURE` for why `Ioctl::classic` is used.
pub const LOOP_SET_STATUS64: Ioctl<Write, &LoopInfo> = unsafe { Ioctl::classic(0x4C04) };

// SAFETY: `LOOP_GET_STATUS64 = 0x4C05` is `_IO(0x4C, 0x05)`. The kernel
// fills a `LoopInfo` (`struct loop_info64`) through the arg pointer, so the
// direction is `WriteRead` and the arg is passed as `&mut`. Every bit pattern
// the kernel can leave behind is a legal value of `LoopInfo` (all
// integer/byte-array fields). See `LOOP_CONFIGURE` for why `Ioctl::classic`
// is used.
pub const LOOP_GET_STATUS64: Ioctl<WriteRead, &LoopInfo> = unsafe { Ioctl::classic(0x4C05) };

// Scalar / no-arg loop ioctls, `_IO(0x4C, n)`. iocuddle's `Write<c_int>`
// passes the argument by value and `Write<c_void>` passes none; both return
// the kernel's non-negative result (a loop number for `CTL_ADD`/`GET_FREE`).
// SAFETY (all below): each request is the exact `_IO(0x4C, n)` number from
// `<linux/loop.h>`, and each kernel handler takes its argument by value (an
// fd, a size, a boolean, or a loop number) — never as a pointer — matching
// the `c_int`/`c_void` argument type.

/// Detach the backing file.
pub const LOOP_CLR_FD: Ioctl<Write, c_void> = unsafe { Ioctl::classic(0x4C01) };

/// Swap the backing file; arg is the new fd.
pub const LOOP_CHANGE_FD: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C06) };

/// Re-read the backing file's size.
pub const LOOP_SET_CAPACITY: Ioctl<Write, c_void> = unsafe { Ioctl::classic(0x4C07) };

/// Toggle direct I/O; arg is 0 or 1.
pub const LOOP_SET_DIRECT_IO: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C08) };

/// Set the logical block size; arg is the size.
pub const LOOP_SET_BLOCK_SIZE: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C09) };

/// Create `/dev/loopN`; arg is the number, which is also returned.
pub const LOOP_CTL_ADD: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C80) };

/// Remove `/dev/loopN`; arg is the number.
pub const LOOP_CTL_REMOVE: Ioctl<Write, c_int> = unsafe { Ioctl::classic(0x4C81) };

/// Return a free loop number, adding a device if needed.
pub const LOOP_CTL_GET_FREE: Ioctl<Write, c_void> = unsafe { Ioctl::classic(0x4C82) };

// `#[repr(C)]` mirror of `struct loop_config` from `<linux/loop.h>`
#[repr(C)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct LoopConfig {
    fd: u32,
    block_size: u32,
    info: LoopInfo,
    __reserved: [u64; 8],
}

const _: () = {
    assert!(core::mem::size_of::<LoopConfig>() == 304);
    assert!(core::mem::offset_of!(LoopConfig, fd) == 0);
    assert!(core::mem::offset_of!(LoopConfig, block_size) == 4);
    assert!(core::mem::offset_of!(LoopConfig, info) == 8);
    assert!(core::mem::offset_of!(LoopConfig, __reserved) == 240);
};

// `#[repr(C)]` mirror of `struct loop_info64` from `<linux/loop.h>`.
#[repr(C)]
#[derive(Clone, Copy, Debug, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub struct LoopInfo {
    pub device: u64,  // ioctl ro
    pub inode: u64,   // ioctl ro
    pub rdevice: u64, // ioctl ro
    pub offset: u64,
    pub sizelimit: u64, // bytes, 0 == max
    pub number: u32,    // ioctl ro
    pub encrypt_type: u32,
    pub encrypt_key_size: u32, // ioctl wo
    pub flags: u32,            // ioctl rw (ro before 2.6.25)
    pub file_name: [u8; LO_NAME_SIZE],
    pub crypt_name: [u8; LO_NAME_SIZE],
    pub encrypt_key: [u8; LO_KEY_SIZE], // ioctl wo
    pub init: [u64; 2],
}

impl LoopInfo {
    pub fn config(self, fd: u32, block_size: u32) -> LoopConfig {
        LoopConfig {
            fd,
            block_size,
            info: self,
            __reserved: [0; 8],
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<LoopInfo>() == 232);
    assert!(core::mem::offset_of!(LoopInfo, device) == 0);
    assert!(core::mem::offset_of!(LoopInfo, offset) == 24);
    assert!(core::mem::offset_of!(LoopInfo, sizelimit) == 32);
    assert!(core::mem::offset_of!(LoopInfo, number) == 40);
    assert!(core::mem::offset_of!(LoopInfo, flags) == 52);
    assert!(core::mem::offset_of!(LoopInfo, file_name) == 56);
    assert!(core::mem::offset_of!(LoopInfo, init) == 216);
};
