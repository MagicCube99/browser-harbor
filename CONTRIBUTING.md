# Contributing

[简体中文](CONTRIBUTING.zh-CN.md) · English

## Development environment

- Windows 10/11 x64.
- Rust 1.99.1.
- Visual Studio Build Tools with the C++ desktop workload.
- Git and PowerShell.

## Build and test

From the repository root:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo build --release
```

Run the complete Windows quality gate with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

Live network tests are opt-in because they download large browser packages:

```powershell
cargo test --test providers_live -- --ignored --nocapture
cargo test --test live_artifacts -- --ignored --nocapture --test-threads=1
cargo test --test live_install -- --ignored --nocapture --test-threads=1
```

## Pull requests

- Keep one logical change per pull request.
- Add regression tests for parsers, installation transactions, path handling, and trust boundaries.
- Preserve the `MIT OR Apache-2.0` licensing model.
- Update `THIRD_PARTY_NOTICES.md` when runtime components or licenses change.
- Do not commit downloaded browsers, generated EXEs, caches, secrets, or personal absolute paths.

## Browser support claims

Do not remove an experimental Chrome++ label based only on compilation or extraction success. Real-browser data placement, login retention, update retention, and configuration repair behavior must be verified.
