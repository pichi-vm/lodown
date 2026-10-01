// SPDX-License-Identifier: Apache-2.0

//! Binding a backing file, and the operations on a bound device.

#[path = "common/backing.rs"]
mod backing;
mod common;

use std::ffi::CStr;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read as _, Seek as _, SeekFrom, Write as _};
use std::num::NonZero;
use std::time::Duration;

use lodown::{Configurable, Device, Name, Writable};

use backing::{BACKING_SIZE, BackingFile};
use common::open_control;

/// What every loop ioctl reports for an unbound device.
const ENXIO: i32 = 6;

const OFFSET: u64 = 64 * 1024;
const SIZE_LIMIT: u64 = 1024 * 1024;

/// Regression test: the device node must be opened read-write.
///
/// `loop_configure` silently ORs in `LO_FLAGS_READ_ONLY` for an `O_RDONLY`
/// node, yielding a read-only device with no error to notice it by.
#[test]
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn configure_defaults_leave_a_writable_device() {
    let control = open_control();

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
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn configure_round_trips_every_field() {
    let control = open_control();

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
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn read_only_device_rejects_writes() {
    let control = open_control();

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

/// Waits for the kernel to finish tearing our binding down.
fn await_detach(number: u32, our_inode: u64, why: &str) {
    for _ in 0..100 {
        if binding_is_gone(number, our_inode) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{why}");
}

#[test]
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn autoclear_detaches_when_the_last_handle_closes() {
    let control = open_control();

    let backing = BackingFile::create("autoclear");
    let config = Configurable {
        writable: Writable {
            autoclear: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let device = control
        .attach(&backing.file, 0, config)
        .expect("attach a loop device");
    let number = device.status().expect("status").number;
    assert!(device.status().is_ok(), "bound while the handle is open");

    // Closing our only handle is what should trigger the detach.
    drop(device);

    // Reclamation by another test is safe: a different inode also proves our
    // binding detached, so this test does not require an exclusive number.
    await_detach(
        number,
        backing.inode(),
        "autoclear never detached the backing file",
    );
}

/// `clear` defers to autoclear when another opener holds the device.
#[test]
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn clear_defers_to_autoclear_while_another_handle_is_open() {
    let control = open_control();

    let backing = BackingFile::create("clear-defer");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");
    let number = device.status().expect("status").number;

    let holder = Device::open(number).expect("second handle");
    device.clear().expect("clear reports success either way");

    let status = holder.status().expect("still bound, not detached");
    assert_eq!(status.inode, backing.inode(), "binding must still be up");
    assert!(status.autoclear, "clear must arm autoclear when it defers");

    drop(holder);
    await_detach(number, backing.inode(), "deferred detach never landed");
}

/// A [`Device`] is itself an open handle, so it holds autoclear off.
#[test]
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn a_device_handle_holds_an_autoclear_device_bound() {
    let control = open_control();

    let backing = BackingFile::create("handle-holds");
    let device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");
    let number = device.status().expect("status").number;

    let mut writable = Writable::from(device.status().expect("status"));
    writable.autoclear = true;
    device.set_status(writable).expect("arm autoclear");

    let inspector = Device::open(number).expect("open while held");
    assert!(inspector.status().expect("status").autoclear);
    assert_eq!(
        inspector.status().expect("status").inode,
        backing.inode(),
        "the binding must survive while a handle is open"
    );
    drop(inspector);

    assert!(
        !binding_is_gone(number, backing.inode()),
        "the Device handle alone must keep the binding up"
    );

    drop(device);
    await_detach(
        number,
        backing.inode(),
        "autoclear never detached the backing file",
    );
}

#[test]
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn change_swaps_the_file_on_a_read_only_device() {
    let control = open_control();

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
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn set_direct_io_toggles() {
    let control = open_control();

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
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn set_capacity_and_block_size_are_accepted() {
    let control = open_control();

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

/// Reads and writes on a `Device` reach the backing file's bytes.
///
/// The whole point of a loop device, so the [`Read`]/[`Write`]/[`Seek`]
/// impls must go through the block layer rather than anywhere else.
///
/// Note `Write::flush` is a no-op for a file-backed handle: it pushes no
/// further than the page cache, and only `sync_data`/`sync_all` (or a
/// detach) writes through to the backing file. Those live on [`File`],
/// reached through `AsRef<File>`.
#[test]
#[ignore = "requires root or CAP_SYS_ADMIN"]
fn device_reads_and_writes_the_backing_files_bytes() {
    let control = open_control();

    let backing = BackingFile::create("blockio");
    let mut device = control
        .attach(&backing.file, 0, Configurable::default())
        .expect("attach a loop device");

    let pattern = [0xAB_u8; 512];
    device
        .write_all(&pattern)
        .expect("write via the loop device");
    device
        .as_ref()
        .sync_all()
        .expect("sync through to the backing file");

    device.rewind().expect("rewind");
    let mut read_back = [0_u8; 512];
    device.read_exact(&mut read_back).expect("read back");
    assert_eq!(
        read_back, pattern,
        "the loop device must return what we wrote"
    );

    let mut shared: &Device = &device;
    shared.seek(SeekFrom::Start(0)).expect("seek via &Device");
    let mut via_shared = [0_u8; 4];
    shared
        .read_exact(&mut via_shared)
        .expect("read via &Device");
    assert_eq!(via_shared, [0xAB; 4]);

    shared.rewind().expect("rewind via &Device");
    shared.write_all(&[0xCD; 4]).expect("write via &Device");
    shared.flush().expect("flush via &Device");
    device.flush().expect("flush is a no-op but must not fail");
    device.rewind().expect("rewind");
    let mut after = [0_u8; 4];
    device.read_exact(&mut after).expect("read back");
    assert_eq!(after, [0xCD; 4]);

    device.rewind().expect("rewind");
    device.write_all(&pattern).expect("rewrite the pattern");
    device.as_ref().sync_all().expect("sync");

    device.clear().expect("detach");

    let mut check = std::fs::File::open(backing.path()).expect("reopen backing file");
    let mut from_file = [0_u8; 512];
    check.read_exact(&mut from_file).expect("read backing file");
    assert_eq!(
        from_file, pattern,
        "writes through the loop device must land in the backing file"
    );
}
