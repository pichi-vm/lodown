// SPDX-License-Identifier: Apache-2.0

//! Creating, removing, and claiming loop numbers.

use std::io::ErrorKind;

use lodown::Device;

#[test]
fn get_free_returns_a_real_node() {
    let Some(control) = crate::common::open_control() else {
        return;
    };

    // A parallel test can remove the claimed device before we open it, so
    // retry rather than assert on a single draw.
    for _ in 0..100 {
        let number = control.get_free().expect("get_free");
        match Device::open(number) {
            Ok(_) => return,
            Err(e) if crate::common::raced(&e) => {}
            Err(other) => panic!("the claimed node must open: {other}"),
        }
    }
    panic!("every claimed number was removed by a parallel test");
}

/// Numbers too large for `c_int` are rejected, not wrapped negative.
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
