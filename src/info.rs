// SPDX-License-Identifier: Apache-2.0

//! Device state, split by when each field can be changed.
//!
//! [`Writable`] ⊂ [`Configurable`] ⊂ [`Readable`]: changeable at any time,
//! fixed once bound, read-only.

use std::num::NonZero;
use std::ops::{Deref, DerefMut};

use zerocopy::FromZeros;

use crate::name::Name;
use crate::uapi::{
    LO_FLAGS_AUTOCLEAR, LO_FLAGS_DIRECT_IO, LO_FLAGS_PARTSCAN, LO_FLAGS_READ_ONLY, LoopInfo,
};

/// What [`Device::set_status`](crate::Device::set_status) can change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Writable {
    /// Where the device starts within the backing file.
    pub offset: u64,

    /// Device size; `None` runs to the end of the backing file.
    pub size_limit: Option<NonZero<u64>>,

    /// A label the kernel stores verbatim and never resolves.
    pub file_name: Name,

    /// Detach on last close (`LO_FLAGS_AUTOCLEAR`).
    pub autoclear: bool,

    /// Scan the backing file for partitions (`LO_FLAGS_PARTSCAN`).
    ///
    /// [`Device::set_status`](crate::Device::set_status) can only turn this
    /// on; clearing it takes a fresh
    /// [`configure`](crate::Device::configure).
    pub partscan: bool,
}

/// What [`Device::configure`](crate::Device::configure) can set.
///
/// [`Writable`] plus the two flags fixed for the life of the binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Configurable {
    /// The fields that stay changeable after binding.
    pub writable: Writable,

    /// Refuse writes (`LO_FLAGS_READ_ONLY`).
    ///
    /// Only ever adds the restriction, and cannot be lifted afterwards: an
    /// `O_RDONLY` backing file or node yields a read-only device regardless.
    pub read_only: bool,

    /// Bypass the page cache (`LO_FLAGS_DIRECT_IO`).
    ///
    /// Silently cleared when the filesystem or the offset/block-size
    /// alignment cannot support it, so read it back — or set it with
    /// [`Device::set_direct_io`](crate::Device::set_direct_io), which fails
    /// loudly.
    pub direct_io: bool,
}

/// What [`Device::status`](crate::Device::status) reports.
///
/// [`Configurable`] plus the identifiers the kernel owns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readable {
    /// The fields a caller can set.
    pub configurable: Configurable,

    /// The backing file's `st_dev`.
    pub device: u64,

    /// The backing file's `st_ino`.
    pub inode: u64,

    /// The backing file's `st_rdev`.
    pub rdevice: u64,

    /// The `N` in `/dev/loopN`.
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
                    file_name: Name::from_field(info.file_name),
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
            file_name: Name::from_field([b'x'; crate::uapi::LO_NAME_SIZE]),
            autoclear: true,
            partscan: true,
        }
    }

    /// `Writable` must emit no flag beyond `AUTOCLEAR` and `PARTSCAN`.
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

    /// "No limit" is a zero `lo_sizelimit`, mapping to `None` and back.
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
        let field: &[u8; crate::uapi::LO_NAME_SIZE] = status.file_name.as_ref();
        assert_eq!(field, &[b'y'; crate::uapi::LO_NAME_SIZE]);
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
