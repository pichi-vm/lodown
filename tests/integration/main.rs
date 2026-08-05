// SPDX-License-Identifier: Apache-2.0

//! Real-kernel integration tests, grouped by subject.
//!
//! One binary rather than one file per subject: every `tests/*.rs` is its own
//! crate, so splitting these would compile [`common`] once per file, run the
//! groups sequentially instead of in parallel, and force
//! `#![allow(dead_code)]` on helpers each binary only partly uses. Modules
//! give the same grouping without that — run one with, say,
//! `cargo test --test integration device::`.
//!
//! Every test here needs `CAP_SYS_ADMIN` and skips cleanly without it.

mod common;

mod control;
mod device;
mod set_status;
