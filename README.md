# lodown

Attach files to Linux loop devices from Rust, without hand-packing ioctl
structs.

Needs Linux, and root (or `CAP_SYS_ADMIN`). Contains no `unsafe` code
outside the ioctl number declarations.

```no_run
use std::fs::OpenOptions;
use lodown::{Configurable, Control};

fn main() -> std::io::Result<()> {
    let control = Control::open()?;
    let backing = OpenOptions::new().read(true).write(true).open("disk.img")?;

    let device = control.attach(&backing, 0, Configurable::default())?;
    println!("attached to /dev/loop{}", device.status()?.number);

    device.clear()?;
    Ok(())
}
```

## Configuration

The kernel describes most loop-device state with `loop_info64`.
`LOOP_GET_STATUS64` and `LOOP_SET_STATUS64` pass it directly, while
`LOOP_CONFIGURE` embeds it in `loop_config` alongside binding parameters.
But its fields are not all valid at all times. Some fields the kernel owns
outright and only reports. Some can only be set while the device is being
bound. Others are accepted by later status updates, although individual
flags can still be one-way. The struct does not distinguish them — its own
header marks fields `/* ioctl r/o */` in a comment — and writing to a field
the current operation does not accept is not an error. The kernel ignores
the value and reports success.

So this crate splits the struct into three types, one per level of access:

- [`Writable`] — fields accepted by `LOOP_SET_STATUS64`; `partscan` can be
  enabled but not disabled.
- [`Configurable`] — those plus fields accepted by `LOOP_CONFIGURE` while
  binding.
- [`Readable`] — those plus kernel-owned fields reported by
  `LOOP_GET_STATUS64`.

Each operation takes the widest type whose fields it accepts, so
configure-only and kernel-owned fields cannot be submitted to
`LOOP_SET_STATUS64`. Field-specific kernel rules still apply. Each type
derefs to the one below, so a [`Readable`] reads every field. Conversion
implementations also let a [`Readable`] be passed to an operation accepting
`Into<Writable>`.

## License

Apache-2.0.
