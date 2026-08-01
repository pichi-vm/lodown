# lodown

A high-level, safe Rust interface to the Linux **loop device** control
ioctls. Create and remove loop devices, attach and detach backing files,
read back their status, and adjust capacity, block size, and direct I/O —
all through typed Rust rather than hand-packed `struct loop_config` buffers.

Built on [`iocuddle`](https://crates.io/crates/iocuddle); the only `unsafe`
in the crate is confined to one module (the ioctl-number declarations plus a
single raw helper for the loop ioctls that take a scalar argument by value).

## Requirements

- **Linux** (the loop driver is a Linux subsystem).
- **`CAP_SYS_ADMIN`** (typically root) to open `/dev/loop-control` and the
  `/dev/loopN` nodes.

## Quick start

```no_run
use std::fs::File;
use lodown::{Config, Control};

fn main() -> Result<(), lodown::Error> {
    let control = Control::open()?;              // /dev/loop-control
    let backing = File::open("disk.img")?;

    // Allocate a free device and configure it in one step.
    let dev = control.attach(&backing, &Config::new().offset(0).read_only(true))?;

    let status = dev.status()?;
    println!("/dev/loop{} — offset {}, read_only {}",
             dev.number(), status.offset(), status.is_read_only());

    dev.detach()?;                               // LOOP_CLR_FD
    Ok(())
}
```

## The model

- **`Control`** — the `/dev/loop-control` fd; a factory for devices:
  `open`, `add`, `remove`, `get_free`, and the `attach` convenience
  (`get_free` + `LOOP_CONFIGURE`).
- **`LoopDevice`** — a handle to an opened `/dev/loopN`, remembering its
  number. Everything else lives here: `configure`, `detach`, `status`,
  `set_capacity`, `set_direct_io`, `set_block_size`, `change_fd`.
- **`Config`** — a fluent builder for the settable parameters (`offset`,
  `size_limit`, `read_only`, `autoclear`, `partscan`, `direct_io`,
  `block_size`) applied via `LOOP_CONFIGURE`.
- **`Status`** — a read-only view over `LOOP_GET_STATUS64`.

## Testing

Unit tests run anywhere; the integration tests under `tests/` need root
(they create a real loop device and clean it up):

```sh
cargo test              # unit tests; integration tests skip without root
sudo -E cargo test      # full suite, exercising the real ioctls
```

## License

Apache-2.0.
