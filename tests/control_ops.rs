// SPDX-License-Identifier: Apache-2.0

//! Root-gated integration coverage for the control and device operations
//! beyond the basic attach round trip: explicit `add`/`remove`, `change_fd`,
//! `set_direct_io`, `set_capacity`, and the error-op-string surfaced by an
//! ioctl on an unbound device. All tests skip cleanly without root.

mod common;

use lodown::{Config, Error};

const BACKING_SIZE: u64 = 4 * 1024 * 1024; // 4 MiB

/// Pick a loop number unlikely to collide with the system's own devices.
/// `add`/`remove` take an explicit number, so a high value keeps clear of
/// the low-numbered devices a host typically allocates.
fn spare_number() -> u32 {
    // A per-process offset reduces the chance of two concurrent test binaries
    // fighting over the same number.
    1000 + (std::process::id() % 200)
}

#[test]
fn add_remove_round_trip() {
    let Some(control) = common::open_control() else {
        return;
    };
    let n = spare_number();

    let dev = control.add(n).expect("add loop device");
    assert_eq!(dev.number(), n);

    // Adding the same number again must fail (EEXIST).
    assert!(matches!(control.add(n), Err(Error::LoopIoctl { .. })));

    // Drop our handle before removing so the node isn't busy.
    drop(dev);
    control.remove(n).expect("remove loop device");

    // Removing a now-nonexistent number must fail.
    assert!(matches!(control.remove(n), Err(Error::LoopIoctl { .. })));
}

#[test]
fn change_fd_swaps_backing_file() {
    let Some(control) = common::open_control() else {
        return;
    };

    let a = common::BackingFile::create("changefd-a", BACKING_SIZE);
    let b = common::BackingFile::create("changefd-b", BACKING_SIZE);

    // `LOOP_CHANGE_FD` is only valid for a read-only device.
    let dev = common::attach_retrying(&control, &a.file, &Config::new().read_only(true));

    dev.change_fd(&b.file).expect("swap to backing file B");

    // The swap succeeds, but LOOP_CHANGE_FD does not update the recorded
    // backing-file name — status still reports file A. Assert the device is
    // still usable rather than expecting a name change the kernel never makes.
    let status = dev.status().expect("read status after swap");
    assert_eq!(status.number(), dev.number());

    dev.detach().expect("detach");
}

#[test]
fn set_direct_io_toggles() {
    let Some(control) = common::open_control() else {
        return;
    };

    let backing = common::BackingFile::create("directio", BACKING_SIZE);
    let dev = common::attach_retrying(&control, &backing.file, &Config::new());

    // Direct I/O may be unsupported on the backing filesystem (e.g. tmpfs);
    // only assert the observable state when enabling actually succeeds.
    if dev.set_direct_io(true).is_ok() {
        assert!(dev.status().expect("status").is_direct_io());
        dev.set_direct_io(false).expect("disable direct io");
        assert!(!dev.status().expect("status").is_direct_io());
    } else {
        eprintln!("skip: backing filesystem does not support direct I/O");
    }

    dev.detach().expect("detach");
}

#[test]
fn set_capacity_succeeds() {
    let Some(control) = common::open_control() else {
        return;
    };

    let backing = common::BackingFile::create("capacity", BACKING_SIZE);
    let dev = common::attach_retrying(&control, &backing.file, &Config::new());

    dev.set_capacity().expect("set capacity");

    dev.detach().expect("detach");
}

#[test]
fn status_on_unbound_device_reports_op() {
    let Some(control) = common::open_control() else {
        return;
    };
    let n = spare_number() + 1;

    let dev = control.add(n).expect("add loop device");

    // No backing file bound yet, so LOOP_GET_STATUS64 fails with ENXIO (6).
    match dev.status() {
        Err(Error::LoopIoctl { op, source }) => {
            assert_eq!(op, "LOOP_GET_STATUS64");
            assert_eq!(source.raw_os_error(), Some(6), "expected ENXIO");
        }
        other => panic!("expected LoopIoctl error, got {other:?}"),
    }

    drop(dev);
    control.remove(n).expect("remove loop device");
}
