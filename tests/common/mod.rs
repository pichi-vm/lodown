// SPDX-License-Identifier: Apache-2.0

//! Shared helpers for real-kernel integration tests: a root-gated
//! `Control::open()` skip check, and a temporary sparse backing file.
//!
//! Every test binary that `mod common;`s this file compiles the whole thing
//! but may use only part of it — `#![allow(dead_code)]` avoids per-binary
//! false-positive dead-code warnings.
#![allow(dead_code)]

use std::fs::File;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use lodown::{Config, Control, Error, LoopDevice};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Returns `None` (and prints a skip notice) if this process can't open
/// `/dev/loop-control` — i.e. isn't root / doesn't have `CAP_SYS_ADMIN`.
pub(crate) fn open_control() -> Option<Control> {
    if let Ok(control) = Control::open() {
        return Some(control);
    }
    eprintln!("skip: requires root (or CAP_SYS_ADMIN) for /dev/loop-control");
    None
}

/// `Control::attach`, retrying on the `LOOP_CTL_GET_FREE`/`LOOP_CONFIGURE`
/// race (`ErrorKind::ResourceBusy`). `attach` deliberately bakes in no retry
/// policy, so this is the caller-side loop the crate docs describe — and it
/// keeps these integration tests, which run in parallel and thus contend for
/// free loop devices, from flaking against each other.
pub(crate) fn attach_retrying(control: &Control, backing: &File, config: &Config) -> LoopDevice {
    for _ in 0..100 {
        match control.attach(backing, config) {
            Ok(device) => return device,
            Err(Error::LoopIoctl { source, .. }) if source.kind() == ErrorKind::ResourceBusy => {}
            Err(other) => panic!("attach failed: {other}"),
        }
    }
    panic!("attach kept losing the get-free/configure race");
}

/// A temporary sparse file usable as loop-device backing. Deletes itself on
/// drop (best-effort).
pub(crate) struct BackingFile {
    pub(crate) file: File,
    path: PathBuf,
}

impl BackingFile {
    /// Creates a `size_bytes`-sized sparse file open for read/write.
    pub(crate) fn create(name: &str, size_bytes: u64) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lodown-test-{name}-{}-{id}", std::process::id()));
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .expect("create backing file");
        file.set_len(size_bytes).expect("set_len backing file");
        Self { file, path }
    }
}

impl Drop for BackingFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
