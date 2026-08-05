// SPDX-License-Identifier: Apache-2.0

//! Real-kernel integration tests, grouped by subject.
//!
//! One binary rather than one file per subject: each `tests/*.rs` is its own
//! crate, so a split would rebuild [`common`] per file, run the groups
//! sequentially, and force `allow(dead_code)` on partly-used helpers. Run a
//! single group with `cargo test --test integration device::`.
//!
//! Everything here needs `CAP_SYS_ADMIN` and skips without it.

mod common;

mod control;
mod device;
mod set_status;
