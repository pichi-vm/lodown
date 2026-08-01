// SPDX-License-Identifier: Apache-2.0

//! End-to-end loop-device round trip: attach a sparse backing file, read its
//! status, and detach it. Root-gated — skips cleanly without privilege.

mod common;

use lodown::Config;

const BACKING_SIZE: u64 = 4 * 1024 * 1024; // 4 MiB
const OFFSET: u64 = 64 * 1024; // 64 KiB
const SIZE_LIMIT: u64 = 1024 * 1024; // 1 MiB

#[test]
fn attach_status_detach_round_trip() {
    let Some(control) = common::open_control() else {
        return;
    };

    let backing = common::BackingFile::create("roundtrip", BACKING_SIZE);

    let config = Config::new().offset(OFFSET).size_limit(SIZE_LIMIT).read_only(true);
    let dev = common::attach_retrying(&control, &backing.file, &config);

    let status = dev.status().expect("read status");
    assert_eq!(status.number(), dev.number());
    assert_eq!(status.offset(), OFFSET);
    assert_eq!(status.size_limit(), SIZE_LIMIT);
    assert!(status.is_read_only());

    dev.detach().expect("detach backing file");
}
