// SPDX-License-Identifier: Apache-2.0

//! Binding a backing file, and the operations on a bound device.

#[path = "common/backing.rs"]
mod backing;
mod common;

use std::ffi::CStr;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write as _};
use std::num::NonZero;

use lodown::{Configurable, Name, Writable};

use backing::{BACKING_SIZE, BackingFile};
use common::open_control;

const OFFSET: u64 = 64 * 1024;
const SIZE_LIMIT: u64 = 1024 * 1024;

/// Regression test: the device node must be opened read-write.
///
/// `loop_configure` silently ORs in `LO_FLAGS_READ_ONLY` for an `O_RDONLY`
/// node, yielding a read-only device with no error to notice it by.
#[test]
fn configure_defaults_leave_a_writable_device() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("defaults");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");

    let status = device.status().expect("status");
    assert!(
        !status.read_only,
        "a default configure over a read-write backing file must be writable"
    );
    assert_eq!(status.inode, backing.inode());
    assert_eq!(status.offset, 0);
    assert_eq!(status.size_limit, None, "no limit means the whole file");
    assert!(!status.autoclear && !status.partscan);

    device.clear().expect("detach");
}

#[test]
fn configure_round_trips_every_field() {
    let Some(control) = open_control() else {
        return;
    };

    let file_name = Name::new("disk.img").expect("under the length limit");

    let backing = BackingFile::create("allfields");
    let config = Configurable {
        writable: Writable {
            offset: OFFSET,
            size_limit: NonZero::new(SIZE_LIMIT),
            file_name,
            autoclear: false,
            partscan: true,
        },
        read_only: true,
        direct_io: false,
    };
    let device = control
        .attach(&backing.file, 4096, config)
        .expect("attach a loop device");

    let status = device.status().expect("status");
    assert_eq!(status.offset, OFFSET);
    assert_eq!(status.size_limit, NonZero::new(SIZE_LIMIT));
    assert_eq!(
        AsRef::<CStr>::as_ref(&status.file_name).to_bytes(),
        b"disk.img"
    );
    assert!(status.partscan);
    assert!(status.read_only);

    device.clear().expect("detach");
}

/// The block device must reject writes, not merely report the flag.
#[test]
fn read_only_device_rejects_writes() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("readonly");
    let config = Configurable {
        read_only: true,
        ..Default::default()
    };
    let device = control
        .attach(&backing.file, 0, config)
        .expect("attach a loop device");
    let number = device.status().expect("status").number;
    assert!(device.status().expect("status").read_only);

    // The node still opens read-write; it's the write that the kernel stops.
    let mut node = OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!("/dev/loop{number}"))
        .expect("open the node");
    let err = node.write(&[0; 512]).expect_err("write must be refused");
    assert_eq!(err.raw_os_error(), Some(1), "expected EPERM");

    drop(node);
    device.clear().expect("detach");
}

#[test]
fn change_swaps_the_file_on_a_read_only_device() {
    let Some(control) = open_control() else {
        return;
    };

    let a = BackingFile::create("swap-a");
    let b = BackingFile::create("swap-b");

    // `LOOP_CHANGE_FD` is only valid for a read-only device.
    let writable = control
        .attach(&a.file, 0, Configurable::default())
        .expect("attach a loop device");
    assert_eq!(
        writable.change(&b.file).unwrap_err().raw_os_error(),
        Some(22),
        "expected EINVAL on a read-write device"
    );
    writable.clear().expect("detach");

    let config = Configurable {
        read_only: true,
        ..Default::default()
    };
    let device = control
        .attach(&a.file, 0, config)
        .expect("attach a loop device");
    assert_eq!(device.status().expect("status").inode, a.inode());

    device.change(&b.file).expect("swap to backing B");
    assert_eq!(
        device.status().expect("status").inode,
        b.inode(),
        "status must follow the new backing file"
    );

    device.clear().expect("detach");
}

#[test]
fn set_direct_io_toggles() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("directio");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");

    // Direct I/O may be unsupported on the backing filesystem (e.g. tmpfs),
    // which the kernel reports as EINVAL/EOPNOTSUPP; skip only on those. Any
    // other error is a real failure, not an unsupported-filesystem skip.
    match device.set_direct_io(true) {
        Ok(()) => {
            assert!(device.status().expect("status").direct_io);
            device.set_direct_io(false).expect("disable direct io");
            assert!(!device.status().expect("status").direct_io);
        }
        Err(e) if matches!(e.raw_os_error(), Some(22 | 95)) => {
            eprintln!("skip: backing filesystem does not support direct I/O");
        }
        Err(other) => panic!("set_direct_io failed unexpectedly: {other}"),
    }

    device.clear().expect("detach");
}

#[test]
fn set_capacity_and_block_size_are_accepted() {
    let Some(control) = open_control() else {
        return;
    };

    let backing = BackingFile::create("capacity");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");

    backing
        .file
        .set_len(BACKING_SIZE * 2)
        .expect("grow the backing file");
    device.set_capacity().expect("set capacity");
    device.set_block_size(512).expect("set block size");

    // A block size that can't fit in a `c_int` is rejected before the ioctl.
    assert_eq!(
        device.set_block_size(u32::MAX).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );

    device.clear().expect("detach");
}
