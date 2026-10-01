# Contributing

## Running the tests

Integration tests that bind real loop devices need root or `CAP_SYS_ADMIN`.
Without those privileges they skip rather than fail:

```sh
cargo test              # device-dependent tests skip
sudo -E cargo test      # device-dependent tests run; ignored tests still skip
```

Set `LODOWN_REQUIRE_ROOT=1` to turn a skip into a failure, so a CI job that
loses its privileges says so instead of passing vacuously.

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
sudo -E LODOWN_REQUIRE_ROOT=1 cargo test --locked
```

All five must be clean. Add a test for any behaviour you change: when fixing
a bug, one that would have failed without the fix.
