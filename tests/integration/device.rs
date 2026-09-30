// SPDX-License-Identifier: Apache-2.0

//! Binding a backing file, and the operations on a bound device.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write as _};
use std::num::NonZero;
use std::time::Duration;

use lodown::{Configurable, Device, Name, Writable};

use crate::common::{BACKING_SIZE, BackingFile, ENXIO, attach, open_control};

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
    let (device, number) = attach(&control, &backing.file, 0, Configurable::default());

    let status = device.status().expect("status");
    assert!(
        !status.read_only,
        "a default configure over a read-write backing file must be writable"
    );
    assert_eq!(status.number, number);
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

    let mut file_name = Name::default();
    file_name[..8].copy_from_slice(b"disk.img");

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
    let (device, number) = attach(&control, &backing.file, 4096, config);

    let status = device.status().expect("status");
    assert_eq!(status.number, number);
    assert_eq!(status.offset, OFFSET);
    assert_eq!(status.size_limit, NonZero::new(SIZE_LIMIT));
    assert_eq!(&status.file_name[..8], b"disk.img");
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
    let (device, number) = attach(&control, &backing.file, 0, config);
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

/// Has the kernel torn our binding down yet?
///
/// Mid-teardown the device sits in `Lo_rundown`, which `lo_open` rejects with
/// `ENXIO`, so a failed *open* means "not settled yet" rather than
/// "detached". A different inode means a concurrent test reclaimed it, which
/// equally proves our binding is gone.
fn binding_is_gone(number: u32, our_inode: u64) -> bool {
    match Device::open(number) {
        Err(e) if e.raw_os_error() == Some(ENXIO) => false,
        Err(e) => panic!("re-open failed: {e}"),
        Ok(device) => match device.status() {
            Err(e) if e.raw_os_error() == Some(ENXIO) => true,
            Err(e) => panic!("status failed: {e}"),
            Ok(status) => status.inode != our_inode,
        },
    }
}

#[test]
fn autoclear_detaches_when_the_last_handle_closes() {
    let Some(control) = open_control() else {
        return;
    };

    // Own the number outright: a device from `get_free` can be reclaimed by a
    // parallel test the moment autoclear releases it, which would leave this
    // test watching someone else's binding.
    let number = crate::common::spare(1);
    control.add(number).expect("add loop device");

    let backing = BackingFile::create("autoclear");
    let config = Configurable {
        writable: Writable {
            autoclear: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let device = Device::open(number).expect("open device node");
    device
        .configure(&backing.file, 0, config)
        .expect("configure");
    assert!(device.status().is_ok(), "bound while the handle is open");

    // Closing our only handle is what should trigger the detach.
    drop(device);

    // Re-open per attempt rather than holding a handle across the wait — an
    // open handle is itself a user, which would keep autoclear from firing.
    let mut attempts = 0;
    while !binding_is_gone(number, backing.inode()) {
        attempts += 1;
        assert!(attempts < 100, "autoclear never detached the backing file");
        std::thread::sleep(Duration::from_millis(10));
    }

    control.remove(number).expect("remove loop device");
}

#[test]
fn change_swaps_the_file_on_a_read_only_device() {
    let Some(control) = open_control() else {
        return;
    };

    let a = BackingFile::create("swap-a");
    let b = BackingFile::create("swap-b");

    // `LOOP_CHANGE_FD` is only valid for a read-only device.
    let (writable, _) = attach(&control, &a.file, 0, Configurable::default());
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
    let (device, _) = attach(&control, &a.file, 0, config);
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
    let (device, _) = attach(&control, &backing.file, 0, Configurable::default());

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
    let (device, _) = attach(&control, &backing.file, 0, Configurable::default());

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

#[test]
fn unbound_device_reports_enxio() {
    let Some(control) = open_control() else {
        return;
    };

    // Own the number outright — `get_free` doesn't reserve it, so a parallel
    // test could bind it and make these assertions spuriously fail.
    let number = crate::common::spare(2);
    control.add(number).expect("add loop device");
    let device = Device::open(number).expect("open device node");

    assert_eq!(device.status().unwrap_err().raw_os_error(), Some(ENXIO));
    assert_eq!(device.clear().unwrap_err().raw_os_error(), Some(ENXIO));

    control.remove(number).expect("remove loop device");
}
