# Contributing

## Running the tests

Tests that bind real loop devices are ignored by default. Run the ordinary
suite without privileges, and the device tests with root or `CAP_SYS_ADMIN`:

```sh
cargo test --locked
sudo -E cargo test --locked --lib --test control --test device --test set_status -- --ignored
```

Two tests assert things about a loop device nobody else touches — that a
device is unbound, that a removed number stays absent. Neither property can
be held against the rest of the system, so they are `#[ignore]`d and need a
machine where nothing else is using loop devices:

```sh
sudo -E cargo test --test exclusive -- --ignored --test-threads=1
```

A failure there may be a regression, or may be something else on the machine
taking a loop device mid-test. Check `losetup -a` before believing it. If it
fails with `EEXIST`, look for a leaked high-numbered `/dev/loop*` node: a test
binary that was killed rather than failing never got to run `Drop`.

## Minimum supported Rust version

1.89, for `File::lock`. Nothing else needs a version that high. Check with:

```sh
cargo +1.89 test --locked
```

## Before opening a pull request

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
cargo +1.89 test --locked
cargo test --locked
sudo -E cargo test --locked --lib --test control --test device --test set_status -- --ignored
```

All six must be clean. Add a test for any behaviour you change: when fixing
a bug, one that would have failed without the fix.
