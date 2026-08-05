// SPDX-License-Identifier: Apache-2.0

//! What `LOOP_SET_STATUS64` actually honours.
//!
//! The ioctl reports success no matter what you hand it, then applies only
//! the fields in its settable/clearable masks. These tests pin down which
//! ones those are — the behaviour the `Writable` / `Configurable` split
//! exists to encode. Root-gated; every test skips cleanly without privilege.

use std::num::NonZero;

use lodown::{Configurable, Name, Writable};

use crate::common::{BackingFile, attach, open_control};

#[test]
fn honours_offset_size_limit_and_file_name() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("setstatus");
    let (device, _) = attach(&control, &backing.file, 0, Configurable::default());

    let mut file_name = Name::default();
    file_name[..7].copy_from_slice(b"new.img");

    let mut writable = Writable::from(device.status().expect("status"));
    writable.offset = 8192;
    writable.size_limit = NonZero::new(1 << 20);
    writable.file_name = file_name;
    device.set_status(writable).expect("set_status");

    let status = device.status().expect("status after set");
    assert_eq!(status.offset, 8192);
    assert_eq!(status.size_limit, NonZero::new(1 << 20));
    assert_eq!(&status.file_name[..7], b"new.img");

    device.clear().expect("detach");
}

/// `autoclear` is the only flag in `LOOP_SET_STATUS_CLEARABLE_FLAGS`, so it
/// is the only one this ioctl can turn back off.
#[test]
fn sets_and_clears_autoclear() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("autoclear");
    let (device, _) = attach(&control, &backing.file, 0, Configurable::default());

    let mut writable = Writable::from(device.status().expect("status"));
    writable.autoclear = true;
    device.set_status(writable).expect("set autoclear");
    assert!(device.status().expect("status").autoclear);

    writable.autoclear = false;
    device.set_status(writable).expect("clear autoclear");
    assert!(!device.status().expect("status").autoclear);

    device.clear().expect("detach");
}

/// `partscan` is in the settable mask but not the clearable one, so asking
/// to turn it off returns success and does nothing.
#[test]
fn sets_but_cannot_clear_partscan() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("partscan");
    let (device, _) = attach(&control, &backing.file, 0, Configurable::default());

    let mut writable = Writable::from(device.status().expect("status"));
    writable.partscan = true;
    device.set_status(writable).expect("set partscan");
    assert!(device.status().expect("status").partscan);

    writable.partscan = false;
    device.set_status(writable).expect("request partscan clear");
    assert!(
        device.status().expect("status").partscan,
        "LOOP_SET_STATUS64 reports success but cannot clear partscan"
    );

    device.clear().expect("detach");
}

/// `read_only` and `direct_io` are absent from `Writable` precisely because
/// this ioctl masks them off, so a full status round trip must leave them
/// exactly as `configure` set them.
#[test]
fn leaves_the_configure_only_flags_alone() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("configonly");
    let config = Configurable {
        read_only: true,
        ..Default::default()
    };
    let (device, _) = attach(&control, &backing.file, 0, config);

    let before = device.status().expect("status");
    assert!(before.read_only);

    device.set_status(before).expect("set_status");

    let after = device.status().expect("status");
    assert!(
        after.read_only,
        "read_only is configure-only and must survive a set_status round trip"
    );
    assert_eq!(after.direct_io, before.direct_io);

    device.clear().expect("detach");
}
