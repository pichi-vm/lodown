// SPDX-License-Identifier: Apache-2.0

//! Helpers shared by the integration tests.

use lodown::Control;

/// Opens `/dev/loop-control` for a capability-dependent test.
pub(crate) fn open_control() -> Control {
    Control::open().expect("open /dev/loop-control; test requires root or CAP_SYS_ADMIN")
}
