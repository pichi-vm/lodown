// SPDX-License-Identifier: Apache-2.0

//! Shared helpers for the real-kernel integration tests: a root-gated
//! `Control::open()` skip check, an attach helper that retries the
//! `get_free`/`configure` race, and a temporary sparse backing file.

use std::fs::File;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use lodown::{Configurable, Control, Device};

/// `ENXIO` — what every loop ioctl reports for an unbound device.
pub(crate) const ENXIO: i32 = 6;

/// A backing-file size big enough for an offset plus a size limit.
pub(crate) const BACKING_SIZE: u64 = 4 * 1024 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A loop number unlikely to collide with the host's own devices, for tests
/// that need a device no concurrent `get_free` can claim. Tests within a
/// binary run in parallel, so each takes its own `slot`; the per-process
/// stride keeps concurrent test binaries apart.
pub(crate) fn spare(slot: u32) -> u32 {
    1000 + (std::process::id() % 200) * 8 + slot
}

/// Returns `None` (and prints a skip notice) if this process can't open
/// `/dev/loop-control` — i.e. isn't root / doesn't have `CAP_SYS_ADMIN`.
/// Setting `LODOWN_REQUIRE_ROOT` turns the skip below into a failure, so a CI
/// job that loses its privileges reports that instead of a vacuous pass.
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

/// `get_free` + `Device::open` + `configure`, retrying the whole sequence on
/// the race `get_free` documents: it doesn't reserve the number, so a
/// concurrent claimant can leave `configure` failing with `EBUSY`. These
/// tests run in parallel and contend for free devices, so without this they
/// flake against each other.
pub(crate) fn attach(
    control: &Control,
    backing: &File,
    block_size: u32,
    config: Configurable,
) -> (Device, u32) {
    for _ in 0..100 {
        let number = control.get_free().expect("get_free");
        let device = Device::open(number).expect("open device node");
        match device.configure(backing, block_size, config) {
            Ok(()) => return (device, number),
            Err(e) if e.kind() == ErrorKind::ResourceBusy => {}
            Err(other) => panic!("configure failed: {other}"),
        }
    }
    panic!("kept losing the get-free/configure race");
}

/// A temporary sparse file usable as loop-device backing. Deletes itself on
/// drop (best-effort).
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

    /// The backing file's inode, which the kernel reports back as
    /// `Readable::inode`.
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
