// SPDX-License-Identifier: Apache-2.0

//! Errors from the loop-device ioctl layer.

use thiserror::Error;

/// Errors from the loop-device ioctl layer. Operational failures only.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Caller-side misuse (e.g. an overlong backing-file name) caught before
    /// any ioctl was attempted.
    #[error("usage: {0}")]
    Usage(String),

    /// A non-ioctl I/O failure (opening `/dev/loop-control`, opening a
    /// `/dev/loopN` node, etc.).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A loop ioctl itself failed.
    #[error("loop ioctl {op} failed: {source}")]
    LoopIoctl {
        /// The ioctl command name, e.g. `"LOOP_CONFIGURE"`.
        op: &'static str,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
}
