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

The kernel describes a loop device with one struct, `loop_info64`, and
passes it to every ioctl that reads or writes device state. But the fields
are not all valid at all times. Some fields the kernel owns outright and only
reports. Some can only be set while the device is being bound. The rest can
be changed whenever. The struct does not distinguish them — its own header
marks fields `/* ioctl r/o */` in a comment — and writing to a field the
current operation does not accept is not an error. The kernel ignores the
value and reports success.

So this crate splits the struct into three types, one per level of access:

- [`Writable`] — fields you can change at any time.
- [`Configurable`] — those plus the ones fixed while binding.
- [`Readable`] — those plus the ones the kernel owns.

Each operation takes the widest type it can honour, so fields it would ignore
cannot be submitted to it. Field-specific rules still apply: for example,
`partscan` can be enabled but not disabled. Each type derefs to the one below,
so a [`Readable`] reads every field, and a [`Readable`] can be passed where a
[`Writable`] is wanted.

## License

Apache-2.0.

[`Writable`]: https://docs.rs/lodown/latest/lodown/struct.Writable.html
[`Configurable`]: https://docs.rs/lodown/latest/lodown/struct.Configurable.html
[`Readable`]: https://docs.rs/lodown/latest/lodown/struct.Readable.html
