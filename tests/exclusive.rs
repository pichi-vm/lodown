// SPDX-License-Identifier: Apache-2.0

//! Tests that require exclusive use of the loop subsystem.
//!
//! **These are `#[ignore]`d and must stay that way.** Every test here asserts
//! something about a loop number *nobody else touches* — that a device is
//! unbound, or that a number stays absent after removal. Neither property can
//! be reserved against the rest of the system.
//!
//! Run them only on a host with an idle loop subsystem, serially:
//!
//! ```sh
//! sudo -E cargo test --test exclusive -- --ignored --test-threads=1
//! ```

mod common;

use lodown::{Control, Device};

use common::open_control;

const ENXIO: i32 = 6;

fn spare(slot: u32) -> u32 {
    const SPARE_BASE: u32 = 1000;
    SPARE_BASE + (std::process::id() % 200) * 8 + slot
}

struct Node<'a> {
    control: &'a Control,
    number: Option<u32>,
}

impl<'a> Node<'a> {
    fn new(control: &'a Control, number: u32) -> Self {
        Node {
            control,
            number: Some(number),
        }
    }

    fn remove(mut self) -> std::io::Result<()> {
        let number = self.number.take().expect("node is live");
        self.control.remove(number)
    }
}

impl Drop for Node<'_> {
    fn drop(&mut self) {
        if let Some(number) = self.number {
            let _ = self.control.remove(number);
        }
    }
}

/// Regression test: `add` must issue `LOOP_CTL_ADD`, not `LOOP_SET_FD`.
///
/// The wrong request number fails with `ENOSYS` and creates nothing, so this
/// checks the node really appears rather than that the ioctl merely returned.
#[test]
#[ignore = "needs an idle loop subsystem; see module docs"]
fn add_creates_a_usable_node_and_remove_destroys_it() {
    let control = open_control();
    let number = spare(0);
    assert_eq!(control.add(number).expect("add loop device"), number);
    let node = Node::new(&control, number);
    let device = Device::open(number).expect("the freshly added node must open");

    // Adding the same number twice must fail (EEXIST).
    assert!(control.add(number).is_err());

    // The kernel refuses to remove a node while a handle is open.
    drop(device);
    node.remove().expect("remove loop device");

    // Gone: neither openable nor removable a second time (ENODEV).
    assert!(Device::open(number).is_err());
    assert!(control.remove(number).is_err());
}

#[test]
#[ignore = "needs an idle loop subsystem; see module docs"]
fn unbound_device_reports_enxio() {
    let control = open_control();

    // Own the number outright — `get_free` doesn't reserve it, so a parallel
    // test could bind it and make these assertions spuriously fail.
    let number = spare(2);
    control.add(number).expect("add loop device");
    let node = Node::new(&control, number);
    let device = Device::open(number).expect("open device node");

    assert_eq!(device.status().unwrap_err().raw_os_error(), Some(ENXIO));
    assert_eq!(device.clear().unwrap_err().raw_os_error(), Some(ENXIO));

    node.remove().expect("remove loop device");
}
