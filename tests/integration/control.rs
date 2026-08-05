// SPDX-License-Identifier: Apache-2.0

//! `/dev/loop-control`: creating, removing, and claiming loop numbers.
//! Root-gated — every test skips cleanly without privilege.

use std::io::ErrorKind;

use lodown::Device;

/// Regression test: `add` must issue `LOOP_CTL_ADD` (0x4C80).
///
/// Sending `LOOP_SET_FD` (0x4C00) to the control node instead fails with
/// `ENOSYS` and creates nothing, so this asserts the node genuinely appears
/// and can be opened, not merely that the ioctl returned.
#[test]
fn add_creates_a_usable_node_and_remove_destroys_it() {
    let Some(control) = crate::common::open_control() else {
        return;
    };
    let n = crate::common::spare(0);

    assert_eq!(control.add(n).expect("add loop device"), n);
    let device = Device::open(n).expect("the freshly added node must open");

    // Adding the same number twice must fail (EEXIST).
    assert!(control.add(n).is_err());

    // The kernel refuses to remove a node while a handle is open.
    drop(device);
    control.remove(n).expect("remove loop device");

    // Gone: neither openable nor removable a second time (ENODEV).
    assert!(Device::open(n).is_err());
    assert!(control.remove(n).is_err());
}

#[test]
fn get_free_returns_a_real_node() {
    let Some(control) = crate::common::open_control() else {
        return;
    };

    let number = control.get_free().expect("get_free");
    Device::open(number).expect("the claimed node must open");
}

/// Loop numbers cross the ioctl boundary as `c_int`, so anything that can't
/// fit is rejected here rather than being wrapped into a negative request.
#[test]
fn numbers_too_large_for_c_int_are_rejected() {
    let Some(control) = crate::common::open_control() else {
        return;
    };

    assert_eq!(
        control.add(u32::MAX).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        control.remove(u32::MAX).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
}
