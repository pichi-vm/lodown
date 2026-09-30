// SPDX-License-Identifier: Apache-2.0

//! The fixed-size backing-file name field.

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;

use crate::uapi::LO_NAME_SIZE;

/// The longest name that fits, in bytes.
///
/// The field is 64 bytes, but the kernel always writes a NUL over the last
/// one.
pub const MAX_NAME_LEN: usize = LO_NAME_SIZE - 1;

/// The kernel's `lo_file_name` field.
///
/// Usually the backing file's path, though the kernel never reads it. Any
/// bytes will do.
///
/// Borrow a `Name` as an [`OsStr`] to get it back. Borrowing as a [`CStr`]
/// works too, but stops at the first NUL, and as `[u8; 64]` gives the field
/// with its padding.
///
/// ```
/// use std::ffi::OsStr;
/// use lodown::Name;
///
/// let name = Name::new("disk.img").expect("short enough");
/// let text: &OsStr = name.as_ref();
///
/// assert_eq!(text, OsStr::new("disk.img"));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Name([u8; LO_NAME_SIZE]);

impl Name {
    /// Builds a name, or `None` past [`MAX_NAME_LEN`] bytes.
    ///
    /// The kernel would truncate an over-long name rather than refuse it,
    /// so this refuses instead.
    ///
    /// ```
    /// use std::path::Path;
    /// use lodown::{MAX_NAME_LEN, Name};
    ///
    /// assert!(Name::new(Path::new("/tmp/disk.img")).is_some());
    /// assert!(Name::new("x".repeat(MAX_NAME_LEN + 1)).is_none());
    /// ```
    pub fn new(name: impl AsRef<OsStr>) -> Option<Self> {
        let bytes = name.as_ref().as_bytes();
        if bytes.len() > MAX_NAME_LEN {
            return None;
        }

        let mut field = [0; LO_NAME_SIZE];
        field[..bytes.len()].copy_from_slice(bytes);
        Some(Name(field))
    }

    /// Wraps a field the kernel filled in.
    ///
    /// Cannot fail, unlike [`new`](Self::new): the kernel only ever reports
    /// a name it already accepted.
    pub(crate) fn from_field(field: [u8; LO_NAME_SIZE]) -> Self {
        Name(field)
    }

    /// The name without its trailing NUL padding.
    fn trimmed(&self) -> &OsStr {
        let end = self.0.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        OsStr::from_bytes(&self.0[..end])
    }

    /// Never fails: [`new`](Self::new) always leaves the last byte NUL.
    fn c_str(&self) -> &CStr {
        CStr::from_bytes_until_nul(&self.0).expect("the last byte is always NUL")
    }
}

impl Default for Name {
    fn default() -> Self {
        Name([0; LO_NAME_SIZE])
    }
}

/// Shows the name, not the padding.
impl std::fmt::Debug for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.trimmed(), f)
    }
}

/// The name, less the padding.
impl AsRef<OsStr> for Name {
    fn as_ref(&self) -> &OsStr {
        self.trimmed()
    }
}

/// Stops at the first NUL, so a name containing one comes back short.
/// Borrow as an [`OsStr`] for all of it.
impl AsRef<CStr> for Name {
    fn as_ref(&self) -> &CStr {
        self.c_str()
    }
}

impl AsRef<[u8; LO_NAME_SIZE]> for Name {
    fn as_ref(&self) -> &[u8; LO_NAME_SIZE] {
        &self.0
    }
}

impl From<Name> for [u8; LO_NAME_SIZE] {
    fn from(name: Name) -> Self {
        name.0
    }
}
