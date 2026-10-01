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

The kernel stores most loop-device state in `loop_info64`, embedding it in
`loop_config` while binding. Its fields do not all have the same write rules:
some the kernel owns, some can only be set while binding, and others belong
to later status updates. Individual flags can still be one-way. The structs
do not enforce those rules, and the kernel may ignore an invalid write while
reporting success.

So this crate splits the struct into three types, one per level of access:

- [`Writable`] — fields accepted by later status updates.
- [`Configurable`] — those plus fields set while binding.
- [`Readable`] — those plus fields the kernel owns.

Each operation takes the matching type, so fields it never accepts cannot be
submitted to it. Field-specific rules still apply: for example, `partscan`
can be enabled but not disabled. Each type derefs to the one below, so a
[`Readable`] reads every field, and a [`Readable`] can be passed where a
[`Writable`] is wanted.

## License

Apache-2.0.

[`Writable`]: https://docs.rs/lodown/latest/lodown/struct.Writable.html
[`Configurable`]: https://docs.rs/lodown/latest/lodown/struct.Configurable.html
[`Readable`]: https://docs.rs/lodown/latest/lodown/struct.Readable.html
