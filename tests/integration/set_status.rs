// SPDX-License-Identifier: Apache-2.0

//! What `LOOP_SET_STATUS64` actually honours.
//!
//! It reports success whatever it is handed, then applies only the fields in
//! its settable and clearable masks. These pin down which ones those are —
//! the behaviour the `Writable` / `Configurable` split encodes.

use std::ffi::CStr;
use std::num::NonZero;

use lodown::{Configurable, Name, Writable};

use crate::common::{BackingFile, open_control};

#[test]
fn honours_offset_size_limit_and_file_name() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("setstatus");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");

    let file_name = Name::new("new.img").expect("under the length limit");

    let mut writable = Writable::from(device.status().expect("status"));
    writable.offset = 8192;
    writable.size_limit = NonZero::new(1 << 20);
    writable.file_name = file_name;
    device.set_status(writable).expect("set_status");

    let status = device.status().expect("status after set");
    assert_eq!(status.offset, 8192);
    assert_eq!(status.size_limit, NonZero::new(1 << 20));
    assert_eq!(
        AsRef::<CStr>::as_ref(&status.file_name).to_bytes(),
        b"new.img"
    );

    device.clear().expect("detach");
}

/// `autoclear` is the only flag this ioctl can turn back off.
#[test]
fn sets_and_clears_autoclear() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("autoclear");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");

    let mut writable = Writable::from(device.status().expect("status"));
    writable.autoclear = true;
    device.set_status(writable).expect("set autoclear");
    assert!(device.status().expect("status").autoclear);

    writable.autoclear = false;
    device.set_status(writable).expect("clear autoclear");
    assert!(!device.status().expect("status").autoclear);

    device.clear().expect("detach");
}

/// `partscan` is settable but not clearable, so clearing is a silent no-op.
#[test]
fn sets_but_cannot_clear_partscan() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("partscan");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");

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

/// A status round trip must leave the configure-only flags untouched.
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
    let device = control
        .attach(&backing.file, 0, config)
        .expect("attach a loop device");

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
