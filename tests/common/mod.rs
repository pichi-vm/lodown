// SPDX-License-Identifier: Apache-2.0

//! Helpers shared by the integration tests.

use lodown::Control;

/// Opens `/dev/loop-control`, or `None` when unprivileged.
///
/// `LODOWN_REQUIRE_ROOT` turns the skip into a failure, so a CI job that
/// loses its privileges reports that instead of passing vacuously.
pub(crate) fn open_control() -> Option<Control> {
    match Control::open() {
        Ok(control) => Some(control),
        Err(error) => {
            assert!(
                std::env::var_os("LODOWN_REQUIRE_ROOT").is_none(),
                "LODOWN_REQUIRE_ROOT is set, but /dev/loop-control could not be opened: {error}"
            );
            eprintln!("skip: requires root (or CAP_SYS_ADMIN) for /dev/loop-control");
            None
        }
    }
}
