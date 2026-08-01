// SPDX-License-Identifier: Apache-2.0

//! [`LoopDevice`]: a handle to an opened `/dev/loopN`.
//! [`LoopInfo`]: the `#[repr(C)]` mirror of `struct loop_info64` that
//! iocuddle's `LOOP_GET_STATUS64`/`LOOP_SET_STATUS64` declarations reference.
//! [`Status`]: a read-only view over `LOOP_GET_STATUS64`.

use std::fmt;
use std::fs::File;
use std::os::fd::{AsFd, AsRawFd};

use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout};

use crate::Error;
use crate::config::Config;
use std::os::raw::c_int;

use crate::uapi::{
    LO_FLAGS_AUTOCLEAR, LO_FLAGS_DIRECT_IO, LO_FLAGS_PARTSCAN, LO_FLAGS_READ_ONLY, LO_KEY_SIZE,
    LO_NAME_SIZE, LOOP_CHANGE_FD, LOOP_CLR_FD, LOOP_CONFIGURE, LOOP_GET_STATUS64,
    LOOP_SET_BLOCK_SIZE, LOOP_SET_CAPACITY, LOOP_SET_DIRECT_IO,
};

/// A handle to an opened loop device (`/dev/loopN`). Owns the device node's
/// `File` and remembers its number.
///
/// Dropping a `LoopDevice` closes the node but does *not* detach the backing
/// file — call [`LoopDevice::detach`] for that (or configure the device with
/// [`Config::autoclear`] so the kernel detaches it when the last user
/// closes it).
///
/// A `LoopDevice` exclusively owns its `/dev/loopN` file descriptor and is
/// intentionally not `Clone`.
pub struct LoopDevice {
    number: u32,
    file: File,
}

impl fmt::Debug for LoopDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopDevice")
            .field("number", &self.number)
            .finish_non_exhaustive()
    }
}

impl LoopDevice {
    pub(crate) fn new(number: u32, file: File) -> Self {
        Self { number, file }
    }

    /// This device's loop number `N` (as in `/dev/loopN`).
    pub fn number(&self) -> u32 {
        self.number
    }

    /// `LOOP_CONFIGURE` — bind `backing` to this device and apply `config`
    /// in a single ioctl.
    ///
    /// The resulting device is writable only if `backing` was opened for
    /// writing (`O_RDWR`); an `O_RDONLY` backing file yields a read-only
    /// device regardless of [`Config::read_only`], so open the backing file
    /// with `OpenOptions::new().read(true).write(true)` when write access is
    /// wanted.
    ///
    /// # Errors
    ///
    /// [`Error::Usage`] if `config` holds a value the kernel would reject
    /// (an invalid block size, or a `direct_io` request whose offset or size
    /// limit isn't aligned to the block size). [`Error::LoopIoctl`] if the
    /// kernel rejects the configuration (e.g. the device is already bound, or
    /// `EBUSY`).
    pub fn configure(&self, backing: &File, config: &Config) -> Result<(), Error> {
        config.validate()?;
        let raw = config.to_loop_config(backing);
        LOOP_CONFIGURE
            .ioctl(self.file.as_fd(), &raw)
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_CONFIGURE",
                source,
            })?;
        Ok(())
    }

    /// `LOOP_CLR_FD` — detach the backing file from this device.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the detach (e.g. the
    /// device is still in use — `EBUSY`).
    pub fn detach(&self) -> Result<(), Error> {
        LOOP_CLR_FD
            .ioctl(self.file.as_fd())
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_CLR_FD",
                source,
            })?;
        Ok(())
    }

    /// `LOOP_GET_STATUS64` — read this device's current configuration.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the query (e.g. the device
    /// has no backing file — `ENXIO`).
    pub fn status(&self) -> Result<Status, Error> {
        let mut info = LoopInfo::new_zeroed();
        LOOP_GET_STATUS64
            .ioctl(self.file.as_fd(), &mut info)
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_GET_STATUS64",
                source,
            })?;
        Ok(Status::from_info(&info))
    }

    /// `LOOP_SET_CAPACITY` — make the device re-read its backing file's
    /// current size (after the file has been grown or shrunk).
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the request.
    pub fn set_capacity(&self) -> Result<(), Error> {
        LOOP_SET_CAPACITY
            .ioctl(self.file.as_fd())
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_SET_CAPACITY",
                source,
            })?;
        Ok(())
    }

    /// `LOOP_SET_DIRECT_IO` — enable or disable page-cache-bypassing I/O to
    /// the backing file.
    ///
    /// Direct I/O requires the backing filesystem to support `O_DIRECT` and
    /// the device's offset and size limit to be aligned to its block size.
    /// Unlike the configure path (where the kernel silently clears the flag),
    /// this ioctl returns an error if the backing file or alignment can't
    /// support direct I/O.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the request (e.g. the
    /// backing file or block size doesn't support direct I/O).
    pub fn set_direct_io(&self, enable: bool) -> Result<(), Error> {
        LOOP_SET_DIRECT_IO
            .ioctl(self.file.as_fd(), c_int::from(enable))
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_SET_DIRECT_IO",
                source,
            })?;
        Ok(())
    }

    /// `LOOP_SET_BLOCK_SIZE` — set the device's logical block size in bytes.
    ///
    /// # Errors
    ///
    /// [`Error::Usage`] if `block_size` is neither `0` nor a power of two
    /// between 512 and 4096 (see [`Config::block_size`] for the page-size
    /// caveat). [`Error::LoopIoctl`] if the kernel otherwise rejects it.
    pub fn set_block_size(&self, block_size: u32) -> Result<(), Error> {
        crate::config::validate_block_size(block_size)?;
        // Block size is validated to <= 4096 above, so it fits in c_int.
        #[allow(clippy::cast_possible_wrap)]
        let arg = block_size as c_int;
        LOOP_SET_BLOCK_SIZE
            .ioctl(self.file.as_fd(), arg)
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_SET_BLOCK_SIZE",
                source,
            })?;
        Ok(())
    }

    /// `LOOP_CHANGE_FD` — atomically swap the backing file for `backing`
    /// (only valid for a read-only device backed by a file of the same
    /// size). Note the kernel does not update the recorded backing-file
    /// name, so [`status`](Self::status)'s `file_name` still reflects the
    /// original file after a swap.
    ///
    /// As with [`configure`](Self::configure), the swapped-in device is
    /// writable only if `backing` was opened for writing (`O_RDWR`); an
    /// `O_RDONLY` backing file leaves the device read-only. Open the backing
    /// file with `OpenOptions::new().read(true).write(true)` when write
    /// access is wanted.
    ///
    /// # Errors
    ///
    /// [`Error::LoopIoctl`] if the kernel rejects the swap.
    pub fn change_fd(&self, backing: &File) -> Result<(), Error> {
        LOOP_CHANGE_FD
            .ioctl(self.file.as_fd(), backing.as_raw_fd())
            .map_err(|source| Error::LoopIoctl {
                op: "LOOP_CHANGE_FD",
                source,
            })?;
        Ok(())
    }
}

/// `#[repr(C)]` mirror of `struct loop_info64` from `<linux/loop.h>`. Field
/// order is byte-for-byte identical to the kernel UAPI (sizeof locked at 232
/// bytes), so iocuddle can pass `&mut LoopInfo` as the `LOOP_GET_STATUS64`
/// argument. Fields are private; the kernel fills them, [`Status`] reads them
/// back, and [`LoopInfo::for_config`] builds the subset a configure needs.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable)]
// The shared `lo_` prefix is the kernel's own field naming in
// `<linux/loop.h>`; this struct mirrors it byte-for-byte, so renaming isn't
// an option.
#[allow(clippy::struct_field_names)]
pub(crate) struct LoopInfo {
    lo_device: u64,
    lo_inode: u64,
    lo_rdevice: u64,
    lo_offset: u64,
    lo_sizelimit: u64,
    lo_number: u32,
    lo_encrypt_type: u32,
    lo_encrypt_key_size: u32,
    lo_flags: u32,
    lo_file_name: [u8; LO_NAME_SIZE],
    lo_crypt_name: [u8; LO_NAME_SIZE],
    lo_encrypt_key: [u8; LO_KEY_SIZE],
    lo_init: [u64; 2],
}

const _: () = assert!(core::mem::size_of::<LoopInfo>() == 232);

impl LoopInfo {
    /// Build the `loop_info64` a `LOOP_CONFIGURE` carries: only `lo_offset`,
    /// `lo_sizelimit`, and `lo_flags` are caller-controlled; every other
    /// field is left zeroed for the kernel to fill.
    pub(crate) fn for_config(offset: u64, size_limit: u64, flags: u32) -> Self {
        Self {
            lo_offset: offset,
            lo_sizelimit: size_limit,
            lo_flags: flags,
            ..Self::new_zeroed()
        }
    }

    /// `lo_offset`, for cross-module test assertions on a built config.
    #[cfg(test)]
    pub(crate) fn offset(&self) -> u64 {
        self.lo_offset
    }

    /// `lo_sizelimit`, for cross-module test assertions on a built config.
    #[cfg(test)]
    pub(crate) fn size_limit(&self) -> u64 {
        self.lo_sizelimit
    }

    /// `lo_flags`, for cross-module test assertions on a built config.
    #[cfg(test)]
    pub(crate) fn flags(&self) -> u32 {
        self.lo_flags
    }
}

/// `LOOP_GET_STATUS64`'s fields, as a read-only view. Obtained from
/// [`LoopDevice::status`], never constructed by the caller. Fields are
/// private behind accessors so the struct can grow (it is
/// `#[non_exhaustive]`) without breaking callers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Status {
    offset: u64,
    size_limit: u64,
    number: u32,
    flags: u32,
    file_name: String,
}

impl Status {
    fn from_info(info: &LoopInfo) -> Self {
        let nul = info
            .lo_file_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(info.lo_file_name.len());
        let file_name = String::from_utf8_lossy(&info.lo_file_name[..nul]).into_owned();
        Self {
            offset: info.lo_offset,
            size_limit: info.lo_sizelimit,
            number: info.lo_number,
            flags: info.lo_flags,
            file_name,
        }
    }

    /// Byte offset into the backing file at which the device starts.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Size of the device in bytes; `0` means the full backing file.
    pub fn size_limit(&self) -> u64 {
        self.size_limit
    }

    /// This device's loop number `N`.
    pub fn number(&self) -> u32 {
        self.number
    }

    /// The device is read-only (`LO_FLAGS_READ_ONLY`).
    pub fn is_read_only(&self) -> bool {
        self.flags & LO_FLAGS_READ_ONLY != 0
    }

    /// The device auto-detaches when its last user closes it
    /// (`LO_FLAGS_AUTOCLEAR`).
    pub fn is_autoclear(&self) -> bool {
        self.flags & LO_FLAGS_AUTOCLEAR != 0
    }

    /// The kernel scans the backing file for a partition table
    /// (`LO_FLAGS_PARTSCAN`).
    pub fn is_partscan(&self) -> bool {
        self.flags & LO_FLAGS_PARTSCAN != 0
    }

    /// I/O bypasses the page cache (`LO_FLAGS_DIRECT_IO`).
    pub fn is_direct_io(&self) -> bool {
        self.flags & LO_FLAGS_DIRECT_IO != 0
    }

    /// The raw `lo_flags` word, an escape hatch for `LO_FLAGS_*` bits this
    /// type doesn't model with a dedicated accessor.
    pub fn flags(&self) -> u32 {
        self.flags
    }

    /// The backing file's name as recorded by the kernel (NUL-trimmed,
    /// lossily decoded as UTF-8).
    pub fn file_name(&self) -> &str {
        &self.file_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_info(
        offset: u64,
        size_limit: u64,
        number: u32,
        flags: u32,
        file_name: &[u8],
    ) -> LoopInfo {
        let mut lo_file_name = [0u8; LO_NAME_SIZE];
        lo_file_name[..file_name.len()].copy_from_slice(file_name);
        LoopInfo {
            lo_offset: offset,
            lo_sizelimit: size_limit,
            lo_number: number,
            lo_flags: flags,
            lo_file_name,
            ..LoopInfo::new_zeroed()
        }
    }

    #[test]
    fn status_parses_scalar_fields() {
        let info = synthetic_info(0x1000, 0x2000, 3, 0, b"/tmp/backing.img");
        let status = Status::from_info(&info);
        assert_eq!(status.offset(), 0x1000);
        assert_eq!(status.size_limit(), 0x2000);
        assert_eq!(status.number(), 3);
        assert_eq!(status.file_name(), "/tmp/backing.img");
    }

    #[test]
    fn status_decodes_each_flag() {
        let ro = Status::from_info(&synthetic_info(0, 0, 0, LO_FLAGS_READ_ONLY, b""));
        assert!(ro.is_read_only() && !ro.is_autoclear());

        let ac = Status::from_info(&synthetic_info(0, 0, 0, LO_FLAGS_AUTOCLEAR, b""));
        assert!(ac.is_autoclear() && !ac.is_read_only());

        let ps = Status::from_info(&synthetic_info(0, 0, 0, LO_FLAGS_PARTSCAN, b""));
        assert!(ps.is_partscan());

        let dio = Status::from_info(&synthetic_info(0, 0, 0, LO_FLAGS_DIRECT_IO, b""));
        assert!(dio.is_direct_io());

        let all = Status::from_info(&synthetic_info(
            0,
            0,
            0,
            LO_FLAGS_READ_ONLY | LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_DIRECT_IO,
            b"",
        ));
        assert!(
            all.is_read_only() && all.is_autoclear() && all.is_partscan() && all.is_direct_io()
        );
    }

    #[test]
    fn status_trims_at_first_nul() {
        // Bytes past the NUL are ignored, not folded into the name.
        let info = synthetic_info(0, 0, 0, 0, b"name\0garbage");
        assert_eq!(Status::from_info(&info).file_name(), "name");
    }

    #[test]
    fn status_handles_a_name_filling_the_whole_field() {
        let name = [b'x'; LO_NAME_SIZE];
        let info = synthetic_info(0, 0, 0, 0, &name);
        assert_eq!(Status::from_info(&info).file_name().len(), LO_NAME_SIZE);
    }

    #[test]
    fn status_empty_name_decodes_to_empty_string() {
        let info = synthetic_info(0, 0, 0, 0, b"");
        assert_eq!(Status::from_info(&info).file_name(), "");
    }

    #[test]
    fn status_non_utf8_name_uses_replacement_char() {
        // An invalid UTF-8 byte is decoded lossily to U+FFFD.
        let info = synthetic_info(0, 0, 0, 0, b"na\xffme");
        assert!(Status::from_info(&info).file_name().contains('\u{fffd}'));
    }

    #[test]
    fn status_trims_nul_before_lossy_decode() {
        // NUL-trim happens first, so the trailing 0xff (past the NUL) never
        // reaches the lossy decode: `b"ok\0\xff"` -> `"ok"`.
        let info = synthetic_info(0, 0, 0, 0, b"ok\0\xff");
        assert_eq!(Status::from_info(&info).file_name(), "ok");
    }

    #[test]
    fn status_flags_returns_raw_word() {
        let raw = LO_FLAGS_READ_ONLY | LO_FLAGS_DIRECT_IO;
        let status = Status::from_info(&synthetic_info(0, 0, 0, raw, b""));
        assert_eq!(status.flags(), raw);
    }
}
