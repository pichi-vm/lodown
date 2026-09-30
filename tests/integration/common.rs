// SPDX-License-Identifier: Apache-2.0

//! Shared helpers for the integration tests.

use std::fs::File;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use lodown::Control;

/// What every loop ioctl reports for an unbound device.
pub(crate) const ENXIO: i32 = 6;

/// A backing-file size big enough for an offset plus a size limit.
pub(crate) const BACKING_SIZE: u64 = 4 * 1024 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Loop numbers at or above this belong to a test that owns them outright.
pub(crate) const SPARE_BASE: u32 = 1000;

/// A loop number reserved for one exclusive test.
pub(crate) fn spare(slot: u32) -> u32 {
    SPARE_BASE + (std::process::id() % 200) * 8 + slot
}

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

/// Did a parallel test remove this device out from under us?
///
/// The kernel rejects opening a device mid-teardown with `ENXIO`, and the
/// node is gone entirely (`ENOENT`) once the removal lands.
pub(crate) fn raced(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(ENXIO)) || error.kind() == ErrorKind::NotFound
}

/// A temporary sparse backing file, deleted on drop.
pub(crate) struct BackingFile {
    pub(crate) file: File,
    path: PathBuf,
}

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
