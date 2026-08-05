// SPDX-License-Identifier: Apache-2.0

//! [`Name`]: the kernel's fixed 64-byte backing-file name field.

use std::ops::{Deref, DerefMut};

use crate::uapi::LO_NAME_SIZE;

/// The kernel's fixed 64-byte backing-file name field (`lo_file_name`).
///
/// This wrapper exists so the status types can derive [`Default`]: std
/// implements `Default` for arrays only up to 32 elements, and `LO_NAME_SIZE`
/// is 64. It [`Deref`]s to the raw byte array, so treat it as one — the
/// kernel stores the name verbatim, NUL-padded, and always NUL-terminates
/// what it reports back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Name([u8; LO_NAME_SIZE]);

impl Default for Name {
    fn default() -> Self {
        Name([0; LO_NAME_SIZE])
    }
}

impl From<[u8; LO_NAME_SIZE]> for Name {
    fn from(array: [u8; LO_NAME_SIZE]) -> Self {
        Name(array)
    }
}

impl From<Name> for [u8; LO_NAME_SIZE] {
    fn from(name: Name) -> Self {
        name.0
    }
}

impl Deref for Name {
    type Target = [u8; LO_NAME_SIZE];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Name {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_zeroes() {
        assert_eq!(*Name::default(), [0; LO_NAME_SIZE]);
    }

    #[test]
    fn round_trips_through_the_raw_array() {
        let raw = [b'z'; LO_NAME_SIZE];

        assert_eq!(<[u8; LO_NAME_SIZE]>::from(Name::from(raw)), raw);
    }

    #[test]
    fn derefs_for_reading_and_in_place_edits() {
        let mut name = Name::default();
        name[..4].copy_from_slice(b"loop");

        assert_eq!(&name[..4], b"loop");
        assert_eq!(name.iter().filter(|b| **b != 0).count(), 4);
    }
}
