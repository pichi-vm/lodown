// SPDX-License-Identifier: Apache-2.0

#![doc = include_str!("../README.md")]
//!
//! # Example
//!
//! Allocate a free loop device, back it with a file, read its status, then
//! detach it:
//!
//! ```no_run
//! use std::fs::File;
//! use lodown::{Config, Control};
//!
//! # fn main() -> std::io::Result<()> {
//! let control = Control::open()?;               // needs CAP_SYS_ADMIN
//! let backing = File::open("/path/to/backing.img")?;
//!
//! let dev = control.attach(&backing, &Config::new().offset(0))?;
//! let status = dev.status()?;
//! println!("/dev/loop{} at offset {}", dev.number(), status.offset());
//!
//! dev.detach()?;                                // LOOP_CLR_FD
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod config;
mod control;
mod device;
mod uapi;

pub use config::Config;
pub use control::Control;
pub use device::{Detach, Detached, Guard, LoopDevice, Remove, Removed, Status, Teardown};

/// The handles are safe to share across threads; assert it at compile time
/// so a future field addition can't silently regress it.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Control>();
    assert_send_sync::<LoopDevice>();
};
