// SPDX-License-Identifier: Apache-2.0

//! Building names, and the three ways to read one back.
//!
//! No kernel involved, so these need no privileges.

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use lodown::{MAX_NAME_LEN, Name};

/// The field is one byte longer than the longest name that fits.
const FIELD_LEN: usize = MAX_NAME_LEN + 1;

fn os_str(name: &Name) -> &OsStr {
    name.as_ref()
}

fn c_str(name: &Name) -> &CStr {
    name.as_ref()
}

fn field(name: &Name) -> &[u8; FIELD_LEN] {
    name.as_ref()
}

#[test]
fn a_default_name_is_empty() {
    let name = Name::default();

    assert!(os_str(&name).is_empty());
    assert_eq!(c_str(&name).to_bytes(), b"");
    assert_eq!(field(&name), &[0; FIELD_LEN]);
}

#[test]
fn a_name_pads_out_to_the_full_field() {
    let name = Name::new("loop").expect("short enough");

    assert_eq!(os_str(&name), OsStr::new("loop"));
    assert_eq!(field(&name)[4..], [0; FIELD_LEN - 4]);
}

/// The kernel truncates an over-long name rather than refusing it.
#[test]
fn a_name_too_long_for_the_field_is_rejected() {
    assert!(Name::new("x".repeat(MAX_NAME_LEN)).is_some());
    assert!(Name::new("x".repeat(MAX_NAME_LEN + 1)).is_none());
}

/// The kernel keeps interior NULs, so the three borrows diverge.
#[test]
fn an_interior_nul_separates_the_three_borrows() {
    let name = Name::new(OsStr::from_bytes(b"a\0b")).expect("short enough");

    // The whole name, padding stripped.
    assert_eq!(os_str(&name).as_bytes(), b"a\0b");
    // A C string stops at the first NUL.
    assert_eq!(c_str(&name).to_bytes(), b"a");
    // The raw field keeps the padding as well.
    assert_eq!(&field(&name)[..3], b"a\0b");
    assert_eq!(field(&name)[3..], [0; FIELD_LEN - 3]);
}

/// Trailing padding is indistinguishable from a name that ends in NUL.
#[test]
fn trailing_nuls_are_treated_as_padding() {
    let name = Name::new(OsStr::from_bytes(b"x\0\0")).expect("short enough");

    assert_eq!(os_str(&name).as_bytes(), b"x");
}

#[test]
fn accepts_every_string_type() {
    let expected = Name::new("disk.img").expect("short enough");

    assert_eq!(Name::new(OsStr::new("disk.img")).unwrap(), expected);
    assert_eq!(Name::new(Path::new("disk.img")).unwrap(), expected);
    assert_eq!(Name::new(String::from("disk.img")).unwrap(), expected);
}

#[test]
fn debug_shows_the_name_without_the_padding() {
    let name = Name::new("disk.img").expect("short enough");

    assert_eq!(format!("{name:?}"), r#""disk.img""#);
}
