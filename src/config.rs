// SPDX-License-Identifier: Apache-2.0

//! [`Config`]: a builder for the loop-device settings a caller cares about.
//! [`LoopConfig`]: the `#[repr(C)]` mirror of `struct loop_config` that
//! iocuddle's `LOOP_CONFIGURE` declaration references.

use std::os::fd::AsRawFd;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::Error;
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
    /// be aligned to the [`block_size`](Self::block_size). When enabled with
    /// a nonzero block size, [`configure`](crate::LoopDevice::configure) /
    /// [`attach`](crate::Control::attach) reject unaligned offsets and size
    /// limits up front; if the backing filesystem cannot honor `O_DIRECT`,
    /// the kernel silently clears the flag rather than failing the configure.
    #[must_use]
    pub fn direct_io(mut self, direct_io: bool) -> Self {
        self.direct_io = direct_io;
        self
    }

    /// Logical block size in bytes; `0` (the default) leaves the kernel's
    /// default block size in place.
    ///
    /// A nonzero block size must be a power of two between 512 and the page
    /// size. This crate caps the upper bound at 4096 (the page size on the
    /// common 4 KiB-page architectures); a kernel on a larger-page
    /// architecture would accept more, but such values are rejected here.
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

    /// Validate the values the kernel would reject, returning
    /// [`Error::Usage`] before any ioctl is attempted. Called by
    /// [`crate::LoopDevice::configure`] and [`crate::Control::attach`].
    pub(crate) fn validate(&self) -> Result<(), Error> {
        validate_block_size(self.block_size)?;

        // When direct I/O is requested, the kernel requires `offset` and
        // `size_limit` to be aligned to the block size. A `0` block size
        // leaves the kernel default in place, so alignment can't be checked
        // here; skip the check in that case.
        if self.direct_io && self.block_size != 0 {
            let bs = u64::from(self.block_size);
            if !self.offset.is_multiple_of(bs) {
                return Err(Error::Usage(format!(
                    "direct_io requires offset ({}) to be a multiple of block_size ({})",
                    self.offset, self.block_size
                )));
            }
            if !self.size_limit.is_multiple_of(bs) {
                return Err(Error::Usage(format!(
                    "direct_io requires size_limit ({}) to be a multiple of block_size ({})",
                    self.size_limit, self.block_size
                )));
            }
        }
        Ok(())
    }

    /// Render this config plus a backing-file descriptor into a
    /// `LOOP_CONFIGURE` argument.
    pub(crate) fn to_loop_config(self, backing: &impl AsRawFd) -> LoopConfig {
        // A loop device's backing fd is always a real, non-negative kernel
        // descriptor; the kernel's `loop_config.fd` field is itself a u32.
        #[allow(clippy::cast_sign_loss)]
        let fd = backing.as_raw_fd() as u32;
        LoopConfig {
            fd,
            block_size: self.block_size,
            info: LoopInfo::for_config(self.offset, self.size_limit, self.flags()),
            __reserved: [0; 8],
        }
    }
}

/// Validate a loop-device logical block size, returning [`Error::Usage`]
/// for values the kernel rejects. `0` is valid (the kernel default is kept);
/// any other value must be a power of two between 512 and the page size. The
/// upper bound is capped at 4096 here (the page size on 4 KiB-page
/// architectures); a kernel on a larger-page architecture would accept more.
pub(crate) fn validate_block_size(block_size: u32) -> Result<(), Error> {
    if block_size != 0 && (!(512..=4096).contains(&block_size) || !block_size.is_power_of_two()) {
        return Err(Error::Usage(format!(
            "block_size must be 0 or a power of two between 512 and 4096, got {block_size}"
        )));
    }
    Ok(())
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

    #[test]
    fn block_size_validation_rejects_bad_values() {
        for bad in [300, 1000, 8192] {
            assert!(
                matches!(validate_block_size(bad), Err(Error::Usage(_))),
                "block_size {bad} should be rejected",
            );
        }
    }

    #[test]
    fn block_size_validation_accepts_good_values() {
        for ok in [0, 512, 1024, 2048, 4096] {
            assert!(
                validate_block_size(ok).is_ok(),
                "block_size {ok} should be accepted"
            );
        }
    }

    #[test]
    fn direct_io_rejects_unaligned_offset() {
        let cfg = Config::new().direct_io(true).block_size(512).offset(500);
        assert!(matches!(cfg.validate(), Err(Error::Usage(_))));
    }

    #[test]
    fn direct_io_rejects_unaligned_size_limit() {
        let cfg = Config::new()
            .direct_io(true)
            .block_size(512)
            .size_limit(1000);
        assert!(matches!(cfg.validate(), Err(Error::Usage(_))));
    }

    #[test]
    fn direct_io_accepts_aligned() {
        let cfg = Config::new()
            .direct_io(true)
            .block_size(512)
            .offset(1024)
            .size_limit(2048);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn direct_io_alignment_skipped_when_block_size_zero() {
        // Block size 0 leaves the kernel default; alignment can't be checked.
        let cfg = Config::new().direct_io(true).offset(500).size_limit(1000);
        assert!(cfg.validate().is_ok());
    }
}
