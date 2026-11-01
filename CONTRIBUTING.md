# Contributing

## Running the tests

Tests that bind real loop devices are ignored by default. Run the ordinary
suite without privileges, and the device tests with root or `CAP_SYS_ADMIN`:

```sh
cargo test --locked
sudo -E cargo test --locked --lib --test control --test device --test set_status -- --ignored
```

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
