// SPDX-License-Identifier: Apache-2.0

//! A temporary backing file for tests that bind a device.
//!
//! Every binary that binds a device needs its own copy of this helper.

use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// A backing-file size big enough for an offset plus a size limit.
pub(crate) const BACKING_SIZE: u64 = 4 * 1024 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A temporary sparse backing file, deleted on drop.
pub(crate) struct BackingFile {
    pub(crate) file: File,
    path: PathBuf,
}

#[allow(dead_code, reason = "not every test binary reads the file back")]
impl BackingFile {
    /// Creates a [`BACKING_SIZE`] sparse file open for read/write.
    pub(crate) fn create(name: &str) -> Self {
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
        file.set_len(BACKING_SIZE).expect("set_len backing file");
        Self { file, path }
    }

    /// The inode the kernel reports back.
    pub(crate) fn inode(&self) -> u64 {
        use std::os::unix::fs::MetadataExt;
        self.file.metadata().expect("stat backing file").ino()
    }
}

impl Drop for BackingFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
