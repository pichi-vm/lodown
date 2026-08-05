// SPDX-License-Identifier: Apache-2.0

//! The loop-device state, split into three tiers by which ioctl can touch
//! each field: [`Writable`] (`LOOP_SET_STATUS64`) ⊂ [`Configurable`]
//! (`LOOP_CONFIGURE`) ⊂ [`Readable`] (`LOOP_GET_STATUS64`).
//!
//! The split is not cosmetic. `LOOP_SET_STATUS64` masks the flags it accepts
//! (`LOOP_SET_STATUS_SETTABLE_FLAGS`) and reports success for the rest, so
//! asking it for `read_only` or `direct_io` silently does nothing. Keeping
//! those two fields off [`Writable`] makes that unrepresentable rather than
//! undetectable.

use std::num::NonZero;
use std::ops::{Deref, DerefMut};

use zerocopy::FromZeros;

use crate::name::Name;
use crate::uapi::{
    LO_FLAGS_AUTOCLEAR, LO_FLAGS_DIRECT_IO, LO_FLAGS_PARTSCAN, LO_FLAGS_READ_ONLY, LoopInfo,
};

/// The state [`Device::set_status`](crate::Device::set_status)
/// (`LOOP_SET_STATUS64`) can change on an already-bound device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Writable {
    /// Byte offset into the backing file at which the device starts.
    pub offset: u64,

    /// Device size in bytes; `None` uses the whole backing file from
    /// [`offset`](Self::offset) onward.
    pub size_limit: Option<NonZero<u64>>,

    /// The backing-file name the kernel records and reports back.
    ///
    /// Purely informational — the kernel stores it verbatim and never
    /// resolves it. [`Device::change_backing`](crate::Device::change_backing)
    /// does not update it.
    pub file_name: Name,

    /// Detach the backing file when the device's last user closes it
    /// (`LO_FLAGS_AUTOCLEAR`).
    pub autoclear: bool,

    /// Scan the backing file for a partition table and create partition
    /// devices (`LO_FLAGS_PARTSCAN`).
    ///
    /// `LOOP_SET_STATUS64` can only turn this *on*. Setting it back to
    /// `false` there is silently ignored; clearing it takes a fresh
    /// [`configure`](crate::Device::configure).
    pub partscan: bool,
}

/// The state [`Device::configure`](crate::Device::configure)
/// (`LOOP_CONFIGURE`) can set: every [`Writable`] field, plus the two flags
/// that are fixed for the lifetime of the binding.
///
/// Derefs to [`Writable`], so its fields are reachable directly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Configurable {
    /// The subset `LOOP_SET_STATUS64` can also change later.
    pub writable: Writable,

    /// Refuse writes to the device (`LO_FLAGS_READ_ONLY`).
    ///
    /// This can only *add* the restriction. A device is writable only if both
    /// the backing file and the `/dev/loopN` node were opened read-write, so
    /// an `O_RDONLY` backing file yields a read-only device whatever this is
    /// set to. Once configured, the flag cannot be changed.
    pub read_only: bool,

    /// Bypass the page cache for backing-file I/O (`LO_FLAGS_DIRECT_IO`).
    ///
    /// The kernel silently clears this if the backing filesystem or the
    /// offset/block-size alignment can't support it, so check
    /// [`Readable`] afterwards — or use
    /// [`Device::set_direct_io`](crate::Device::set_direct_io), which fails
    /// loudly instead.
    pub direct_io: bool,
}

/// Everything [`Device::status`](crate::Device::status)
/// (`LOOP_GET_STATUS64`) reports: every [`Configurable`] field, plus the
/// identifiers the kernel owns.
///
/// Derefs to [`Configurable`] (and so to [`Writable`]), so all of it is
/// reachable directly — `status.offset`, `status.read_only`, `status.number`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readable {
    /// The subset `LOOP_CONFIGURE` can set.
    pub configurable: Configurable,

    /// `st_dev` of the backing file.
    pub device: u64,

    /// `st_ino` of the backing file.
    pub inode: u64,

    /// `st_rdev` of the backing file.
    pub rdevice: u64,

    /// This device's loop number `N`, as in `/dev/loopN`.
    pub number: u32,
}

impl Deref for Configurable {
    type Target = Writable;

    fn deref(&self) -> &Self::Target {
        &self.writable
    }
}

impl DerefMut for Configurable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.writable
    }
}

impl Deref for Readable {
    type Target = Configurable;

    fn deref(&self) -> &Self::Target {
        &self.configurable
    }
}

impl DerefMut for Readable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.configurable
    }
}

impl From<Writable> for Configurable {
    fn from(writable: Writable) -> Self {
        Self {
            writable,
            ..Self::default()
        }
    }
}

impl From<Configurable> for Readable {
    fn from(configurable: Configurable) -> Self {
        Self {
            configurable,
            ..Self::default()
        }
    }
}

impl From<Writable> for Readable {
    fn from(writable: Writable) -> Self {
        Self {
            configurable: Configurable::from(writable),
            ..Self::default()
        }
    }
}

impl From<Readable> for Configurable {
    fn from(readable: Readable) -> Self {
        readable.configurable
    }
}

impl From<Configurable> for Writable {
    fn from(configurable: Configurable) -> Self {
        configurable.writable
    }
}

impl From<Readable> for Writable {
    fn from(readable: Readable) -> Self {
        readable.configurable.writable
    }
}

impl From<Writable> for LoopInfo {
    fn from(status: Writable) -> Self {
        let mut info = LoopInfo::new_zeroed();
        info.offset = status.offset;
        info.sizelimit = status.size_limit.map_or(0, NonZero::get);
        info.file_name = status.file_name.into();
        info.flags = (u32::from(status.autoclear) * LO_FLAGS_AUTOCLEAR)
            | (u32::from(status.partscan) * LO_FLAGS_PARTSCAN);
        info
    }
}

impl From<Configurable> for LoopInfo {
    fn from(status: Configurable) -> Self {
        let mut info = LoopInfo::from(status.writable);
        info.flags |= (u32::from(status.read_only) * LO_FLAGS_READ_ONLY)
            | (u32::from(status.direct_io) * LO_FLAGS_DIRECT_IO);
        info
    }
}

impl From<LoopInfo> for Readable {
    fn from(info: LoopInfo) -> Self {
        Self {
            configurable: Configurable {
                writable: Writable {
                    offset: info.offset,
                    size_limit: NonZero::new(info.sizelimit),
                    file_name: info.file_name.into(),
                    autoclear: info.flags & LO_FLAGS_AUTOCLEAR != 0,
                    partscan: info.flags & LO_FLAGS_PARTSCAN != 0,
                },
                read_only: info.flags & LO_FLAGS_READ_ONLY != 0,
                direct_io: info.flags & LO_FLAGS_DIRECT_IO != 0,
            },
            device: info.device,
            inode: info.inode,
            rdevice: info.rdevice,
            number: info.number,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writable() -> Writable {
        Writable {
            offset: 4096,
            size_limit: NonZero::new(8192),
            file_name: [b'x'; crate::uapi::LO_NAME_SIZE].into(),
            autoclear: true,
            partscan: true,
        }
    }

    /// `LOOP_SET_STATUS64` only honours `AUTOCLEAR` and `PARTSCAN`, so a
    /// `Writable` must never contribute any other flag bit.
    #[test]
    fn writable_emits_only_the_set_status_flags() {
        let info = LoopInfo::from(writable());

        assert_eq!(info.offset, 4096);
        assert_eq!(info.sizelimit, 8192);
        assert_eq!(info.file_name, [b'x'; crate::uapi::LO_NAME_SIZE]);
        assert_eq!(info.flags, LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN);
    }

    #[test]
    fn configurable_adds_the_configure_only_flags() {
        let config = Configurable {
            writable: writable(),
            read_only: true,
            direct_io: true,
        };

        assert_eq!(
            LoopInfo::from(config).flags,
            LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_READ_ONLY | LO_FLAGS_DIRECT_IO
        );
    }

    /// The kernel spells "no limit" as a zero `lo_sizelimit`, which must map
    /// to `None` and back.
    #[test]
    fn absent_size_limit_round_trips_through_zero() {
        assert_eq!(LoopInfo::from(Writable::default()).sizelimit, 0);
        assert_eq!(Readable::from(LoopInfo::new_zeroed()).size_limit, None);
    }

    #[test]
    fn readable_decodes_every_field() {
        let mut info = LoopInfo::new_zeroed();
        info.offset = 1;
        info.sizelimit = 2;
        info.device = 3;
        info.inode = 4;
        info.rdevice = 5;
        info.number = 6;
        info.file_name = [b'y'; crate::uapi::LO_NAME_SIZE];
        info.flags =
            LO_FLAGS_READ_ONLY | LO_FLAGS_AUTOCLEAR | LO_FLAGS_PARTSCAN | LO_FLAGS_DIRECT_IO;

        let status = Readable::from(info);

        assert_eq!(status.offset, 1);
        assert_eq!(status.size_limit, NonZero::new(2));
        assert_eq!(status.device, 3);
        assert_eq!(status.inode, 4);
        assert_eq!(status.rdevice, 5);
        assert_eq!(status.number, 6);
        assert_eq!(*status.file_name, [b'y'; crate::uapi::LO_NAME_SIZE]);
        assert!(status.read_only && status.autoclear && status.partscan && status.direct_io);
    }

    #[test]
    fn unset_flags_decode_as_false() {
        let status = Readable::from(LoopInfo::new_zeroed());

        assert!(!status.read_only && !status.autoclear && !status.partscan && !status.direct_io);
    }

    /// Each tier derefs to the next, in both `&` and `&mut` form.
    // Assigning through `DerefMut` is the point here; clippy's suggested
    // struct literal can't express it, since these aren't direct fields.
    #[allow(clippy::field_reassign_with_default)]
    #[test]
    fn tiers_deref_to_the_narrower_tier() {
        let mut status = Readable::default();

        status.offset = 4096; // Readable -> Configurable -> Writable
        status.read_only = true; // Readable -> Configurable
        status.number = 7; // Readable itself

        assert_eq!(status.configurable.writable.offset, 4096);
        assert!(status.configurable.read_only);
        assert_eq!(status.number, 7);

        let mut config = Configurable::default();
        config.partscan = true; // Configurable -> Writable
        assert!(config.writable.partscan);
    }

    #[test]
    fn tiers_convert_in_both_directions() {
        let writable = writable();
        let config = Configurable {
            writable,
            read_only: true,
            direct_io: true,
        };

        // Widening fills the extra fields with their defaults.
        assert_eq!(Configurable::from(writable).writable, writable);
        assert!(!Configurable::from(writable).read_only);
        assert_eq!(Readable::from(writable).writable, writable);
        assert_eq!(Readable::from(config).configurable, config);
        assert_eq!(Readable::from(config).number, 0);

        // Narrowing just drops them.
        assert_eq!(Writable::from(config), writable);
        assert_eq!(Configurable::from(Readable::from(config)), config);
        assert_eq!(Writable::from(Readable::from(config)), writable);
    }
}
