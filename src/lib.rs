// SPDX-License-Identifier: Apache-2.0

#![doc = include_str!("../README.md")]
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
