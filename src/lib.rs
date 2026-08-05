// SPDX-License-Identifier: Apache-2.0

#![doc = include_str!("../README.md")]
//!
//! # Example
//!
//! Allocate a free loop device, back it with a file, read its status, then
//! detach it:
//!
//! ```no_run
//! use std::fs::OpenOptions;
//! use lodown::{Configurable, Control, Device};
//!
//! # fn main() -> std::io::Result<()> {
//! let control = Control::open()?;                       // needs CAP_SYS_ADMIN
//! let backing = OpenOptions::new()
//!     .read(true)
//!     .write(true)
//!     .open("/path/to/backing.img")?;
//!
//! let number = control.get_free()?;
//! let device = Device::open(number)?;
//! device.configure(&backing, 0, Configurable::default())?;
//!
//! let status = device.status()?;                        // LOOP_GET_STATUS64
//! println!("/dev/loop{} at offset {}", status.number, status.offset);
//!
//! device.clear()?;                                      // LOOP_CLR_FD
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(clippy::all, clippy::pedantic)]
#![warn(missing_debug_implementations, missing_docs, unreachable_pub)]
#![warn(rust_2018_idioms)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::wildcard_imports)]

mod control;
mod device;
mod info;
mod name;
mod uapi;

pub use control::Control;
pub use device::Device;
pub use info::{Configurable, Readable, Writable};
pub use name::Name;
