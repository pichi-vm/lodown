# lodown

A high-level, safe Rust interface to the Linux **loop device** control
ioctls. Create and remove loop devices, attach and detach backing files,
read back their status, and adjust capacity, block size, and direct I/O —
all through typed Rust rather than hand-packed `struct loop_config` buffers.

Built on [`iocuddle`](https://crates.io/crates/iocuddle); every ioctl goes
through iocuddle, so the only `unsafe` in the crate is confined to one module
— the ioctl-number declarations (iocuddle's `const` constructors).

## Requirements

- **Linux** (the loop driver is a Linux subsystem).
- **`CAP_SYS_ADMIN`** (typically root) to open `/dev/loop-control` and the
  `/dev/loopN` nodes.

## Quick start

```no_run
use std::fs::OpenOptions;
use lodown::{Configurable, Control, Device};

fn main() -> std::io::Result<()> {
    let control = Control::open()?;             // /dev/loop-control
    let backing = OpenOptions::new().read(true).write(true).open("disk.img")?;

    // Claim a free number and bind the backing file to it. `get_free` does
    // not reserve the device, so a concurrent claimant can make `configure`
    // fail with `EBUSY`; retry from `get_free` if that matters.
    let number = control.get_free()?;
    let device = Device::open(number)?;

    device.configure(&backing, 0, Configurable {
        read_only: true,
        ..Default::default()
    })?;

    let status = device.status()?;
    println!("/dev/loop{} — offset {}, read_only {}",
             status.number, status.offset, status.read_only);

    device.clear()?;                            // detach the backing file
    Ok(())
}
```

## The model

- **`Control`** — the `/dev/loop-control` fd; the node factory: `add`,
  `remove`, `get_free`. It never binds a device.
- **`Device`** — a handle to an opened `/dev/loopN`: `configure`, `clear`,
  `change_backing`, `status`, `set_status`, `set_capacity`, `set_direct_io`,
  `set_block_size`. Dropping it closes the node but does *not* detach the
  backing file — call `clear`, or set `Configurable::autoclear` and let the
  kernel detach on last close.

Device state is split into three tiers, by which ioctl can actually change
each field. `LOOP_SET_STATUS64` masks the flags it accepts and returns
success for the rest, so the split is what keeps a silently-ignored write
from being expressible:

- **`Writable`** — what `set_status` (`LOOP_SET_STATUS64`) can change:
  `offset`, `size_limit`, `file_name`, `autoclear`, `partscan`.
- **`Configurable`** — what `configure` (`LOOP_CONFIGURE`) can set: the
  `Writable` fields plus `read_only` and `direct_io`, which are fixed for the
  lifetime of the binding.
- **`Readable`** — what `status` (`LOOP_GET_STATUS64`) reports: the
  `Configurable` fields plus the kernel-owned `device`, `inode`, `rdevice`,
  and `number`.

Each tier derefs to the one below it, so `status.offset`, `status.read_only`,
and `status.number` all work directly.

## Testing

Unit tests run anywhere; the integration tests under `tests/` need root
(they create a real loop device and clean it up):

```sh
cargo test              # unit tests; integration tests skip without root
sudo -E cargo test      # full suite, exercising the real ioctls
```

## License

Apache-2.0.
