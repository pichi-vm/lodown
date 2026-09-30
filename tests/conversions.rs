// SPDX-License-Identifier: Apache-2.0

//! Descriptor conversions and borrows, and what happens on the wrong file.
//!
//! None of these bind a device, so unlike the rest of the suite they need no
//! privileges: `/dev/null` is enough to prove the plumbing.

use std::fs::File;
use std::os::fd::{AsFd, AsRawFd, IntoRawFd, OwnedFd};

use lodown::{Control, Device};

/// What a loop ioctl reports against a file that is not a loop device.
const ENOTTY: i32 = 25;

fn dev_null() -> File {
    File::open("/dev/null").expect("open /dev/null")
}

/// Wrapping a non-loop file is a runtime error, not unsoundness.
///
/// This is what lets the conversions be `From` rather than `TryFrom`: the
/// ioctls validate the descriptor themselves, so no constructor-time check
/// could do better than letting the first call fail cleanly.
#[test]
fn a_device_over_the_wrong_file_fails_the_ioctl_cleanly() {
    let device = Device::from(dev_null());

    let error = device.status().expect_err("/dev/null is not a loop device");

    assert_eq!(error.raw_os_error(), Some(ENOTTY));
}

#[test]
fn a_control_over_the_wrong_file_fails_the_ioctl_cleanly() {
    let control = Control::from(dev_null());

    let error = control
        .get_free()
        .expect_err("/dev/null is not the loop control node");

    assert_eq!(error.raw_os_error(), Some(ENOTTY));
}

#[test]
fn every_device_conversion_and_borrow_keeps_the_descriptor() {
    let device = Device::from(dev_null());
    let fd = device.as_raw_fd();

    let device = Device::from(File::from(device));
    assert_eq!(device.as_raw_fd(), fd, "File round-trip kept the fd");

    let mut device = Device::from(OwnedFd::from(device));
    assert_eq!(device.as_raw_fd(), fd, "OwnedFd round-trip kept the fd");

    assert_eq!(device.as_fd().as_raw_fd(), fd);
    assert_eq!(AsRef::<File>::as_ref(&device).as_raw_fd(), fd);
    assert_eq!(AsMut::<File>::as_mut(&mut device).as_raw_fd(), fd);

    // `into_raw_fd` surrenders ownership. Reclaiming it needs `unsafe`, which
    // this crate refuses, so the fd stays open for the rest of the process.
    assert_eq!(device.into_raw_fd(), fd);
}

#[test]
fn every_control_conversion_and_borrow_keeps_the_descriptor() {
    let control = Control::from(dev_null());
    let fd = control.as_raw_fd();

    let control = Control::from(File::from(control));
    assert_eq!(control.as_raw_fd(), fd, "File round-trip kept the fd");

    let mut control = Control::from(OwnedFd::from(control));
    assert_eq!(control.as_raw_fd(), fd, "OwnedFd round-trip kept the fd");

    assert_eq!(control.as_fd().as_raw_fd(), fd);
    assert_eq!(AsRef::<File>::as_ref(&control).as_raw_fd(), fd);
    assert_eq!(AsMut::<File>::as_mut(&mut control).as_raw_fd(), fd);

    assert_eq!(control.into_raw_fd(), fd);
}
