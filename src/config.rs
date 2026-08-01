// SPDX-License-Identifier: Apache-2.0

//! [`Config`]: a builder for the loop-device settings a caller cares about.
//! [`LoopConfig`]: the `#[repr(C)]` mirror of `struct loop_config` that
//! iocuddle's `LOOP_CONFIGURE` declaration references.

use std::os::fd::{AsFd, AsRawFd};

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::device::LoopInfo;
use crate::uapi::{LO_FLAGS_AUTOCLEAR, LO_FLAGS_DIRECT_IO, LO_FLAGS_PARTSCAN, LO_FLAGS_READ_ONLY};

/// The settable loop-device parameters, applied via `LOOP_CONFIGURE` when a
/// backing file is attached.
///
/// Build one fluently, then hand it to [`crate::Control::attach`] or
/// [`crate::LoopDevice::configure`]:
///
/// ```
/// use lodown::Config;
///
/// let config = Config::new()
///     .offset(4096)
///     .size_limit(1 << 20)
///     .read_only(true)
///     .block_size(512);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
// The four booleans are the four independent `LO_FLAGS_*` bits, each a
// distinct on/off toggle; a state machine or enum would only obscure them.
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    offset: u64,
    size_limit: u64,
    read_only: bool,
    autoclear: bool,
    partscan: bool,
    direct_io: bool,
    block_size: u32,
}

impl Config {
    /// A default configuration: no offset, no size limit (the whole backing
    /// file), read-write, no auto-clear, no partition scan, buffered I/O,
    /// and the kernel's default block size.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Byte offset into the backing file at which the loop device starts.
    #[must_use]
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = offset;
        self
    }

    /// Size of the loop device in bytes; `0` (the default) uses the full
    /// backing file from `offset` onward.
    #[must_use]
    pub fn size_limit(mut self, size_limit: u64) -> Self {
        self.size_limit = size_limit;
        self
    }

    /// Whether the loop device is read-only (`LO_FLAGS_READ_ONLY`).
    ///
    /// This flag can only *add* the read-only restriction: a loop device is
    /// writable only if its backing file was opened for writing (`O_RDWR`).
    /// An `O_RDONLY` backing file yields a read-only device regardless of
    /// this flag, so open the backing file with
    /// `OpenOptions::new().read(true).write(true)` when write access is
    /// wanted.
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Whether the device auto-detaches when its last user closes it
    /// (`LO_FLAGS_AUTOCLEAR`).
    #[must_use]
    pub fn autoclear(mut self, autoclear: bool) -> Self {
        self.autoclear = autoclear;
        self
    }

    /// Whether the kernel scans the backing file for a partition table and
    /// creates partition devices (`LO_FLAGS_PARTSCAN`).
    #[must_use]
    pub fn partscan(mut self, partscan: bool) -> Self {
        self.partscan = partscan;
        self
    }

    /// Whether I/O bypasses the page cache (`LO_FLAGS_DIRECT_IO`).
    ///
    /// Direct I/O requires the backing filesystem to support `O_DIRECT` and
    /// both [`offset`](Self::offset) and [`size_limit`](Self::size_limit) to
    /// be aligned to the [`block_size`](Self::block_size). If those conditions
    /// aren't met the kernel silently clears the flag rather than failing the
    /// configure, so verify it took effect with
    /// [`Status::is_direct_io`](crate::Status::is_direct_io) afterward.
    #[must_use]
    pub fn direct_io(mut self, direct_io: bool) -> Self {
        self.direct_io = direct_io;
        self
    }

    /// Logical block size in bytes; `0` (the default) leaves the kernel's
    /// default block size in place.
    ///
    /// A nonzero block size must be a power of two between 512 and the page
    /// size; the kernel rejects anything else with `EINVAL` at configure time.
    #[must_use]
    pub fn block_size(mut self, block_size: u32) -> Self {
        self.block_size = block_size;
        self
    }

    /// The `lo_flags` word this config renders to.
    fn flags(self) -> u32 {
        let mut flags = 0;
        if self.read_only {
            flags |= LO_FLAGS_READ_ONLY;
        }
        if self.autoclear {
            flags |= LO_FLAGS_AUTOCLEAR;
        }
        if self.partscan {
            flags |= LO_FLAGS_PARTSCAN;
        }
        if self.direct_io {
            flags |= LO_FLAGS_DIRECT_IO;
        }
        flags
    }

    /// Render this config plus a backing-file descriptor into a
    /// `LOOP_CONFIGURE` argument.
    pub(crate) fn to_loop_config(self, backing: impl AsFd) -> LoopConfig {
        // A loop device's backing fd is always a real, non-negative kernel
        // descriptor; the kernel's `loop_config.fd` field is itself a u32.
        #[allow(clippy::cast_sign_loss)]
        let fd = backing.as_fd().as_raw_fd() as u32;
        LoopConfig {
            fd,
            block_size: self.block_size,
            info: LoopInfo::for_config(self.offset, self.size_limit, self.flags()),
            __reserved: [0; 8],
        }
    }
}

/// `#[repr(C)]` mirror of `struct loop_config` from `<linux/loop.h>` (sizeof
/// locked at 304 bytes), so iocuddle can pass `&LoopConfig` as the
/// `LOOP_CONFIGURE` argument. Fields are private and only constructed by
/// [`Config::to_loop_config`], which cannot produce a representation-invalid
/// value.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable)]
pub(crate) struct LoopConfig {
    fd: u32,
    block_size: u32,
    info: LoopInfo,
    __reserved: [u64; 8],
}

const _: () = assert!(core::mem::size_of::<LoopConfig>() == 304);
// Field offsets are load-bearing for the `struct loop_config` ABI.
const _: () = {
    use core::mem::offset_of;
    assert!(offset_of!(LoopConfig, fd) == 0);
    assert!(offset_of!(LoopConfig, block_size) == 4);
    assert!(offset_of!(LoopConfig, info) == 8);
    assert!(offset_of!(LoopConfig, __reserved) == 240);
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn backing() -> File {
        File::open("/dev/null").expect("/dev/null always exists")
    }

    #[test]
    fn defaults_render_to_zeroed_fields() {
        let file = backing();
        let cfg = Config::new().to_loop_config(&file);
        assert_eq!(cfg.block_size, 0);
        assert_eq!(cfg.info.offset(), 0);
        assert_eq!(cfg.info.size_limit(), 0);
        assert_eq!(cfg.info.flags(), 0);
    }

    #[test]
    fn scalar_fields_land_in_the_right_place() {
        let file = backing();
        let cfg = Config::new()
            .offset(0x1234)
            .size_limit(0x5678)
            .block_size(4096)
            .to_loop_config(&file);
        assert_eq!(cfg.info.offset(), 0x1234);
        assert_eq!(cfg.info.size_limit(), 0x5678);
        assert_eq!(cfg.block_size, 4096);
        assert_eq!(cfg.fd.cast_signed(), file.as_raw_fd());
    }

    #[test]
    fn each_flag_maps_to_its_bit() {
        let file = backing();

        assert_eq!(
            Config::new()
                .read_only(true)
                .to_loop_config(&file)
                .info
                .flags(),
            LO_FLAGS_READ_ONLY
        );
        assert_eq!(
            Config::new()
                .autoclear(true)
                .to_loop_config(&file)
                .info
                .flags(),
            LO_FLAGS_AUTOCLEAR
        );
        assert_eq!(
            Config::new()
                .partscan(true)
                .to_loop_config(&file)
                .info
                .flags(),
            LO_FLAGS_PARTSCAN
        );
        assert_eq!(
            Config::new()
                .direct_io(true)
                .to_loop_config(&file)
                .info
                .flags(),
            LO_FLAGS_DIRECT_IO
        );
    }

    #[test]
    fn all_flags_combine() {
        let file = backing();
        let flags = Config::new()
            .read_only(true)
            .autoclear(true)
            .partscan(true)
            .direct_io(true)
            .to_loop_config(&file)
            .info
            .flags();
        assert_eq!(
            flags,
            LO_FLAGS_READ_ONLY | LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_DIRECT_IO
        );
    }

    #[test]
    fn every_field_lands_together() {
        // Offset, size limit, all four flags, and a nonzero block size set at
        // once — guards against a cross-field clobber in `to_loop_config`.
        let file = backing();
        let raw = Config::new()
            .offset(4096)
            .size_limit(8192)
            .read_only(true)
            .autoclear(true)
            .partscan(true)
            .direct_io(true)
            .block_size(512)
            .to_loop_config(&file);
        assert_eq!(raw.info.offset(), 4096);
        assert_eq!(raw.info.size_limit(), 8192);
        assert_eq!(raw.block_size, 512);
        assert_eq!(
            raw.info.flags(),
            LO_FLAGS_READ_ONLY | LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_DIRECT_IO
        );
        assert_eq!(raw.fd.cast_signed(), file.as_raw_fd());
    }
}
