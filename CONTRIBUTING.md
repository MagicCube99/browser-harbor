# Contributing

[简体中文](CONTRIBUTING.zh-CN.md) · English

## Before changing code

- Use Windows 10/11 x64.
- Install Rust 1.99.1 and Visual Studio Build Tools with the C++ desktop workload.
- Read `docs/ARCHITECTURE.md`.
- Keep the product a portable download/configuration manager; do not add browser launching, default-browser registration, taskbar integration, or telemetry.

## Quality gate

Run from the repository root:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

Live tests download large artifacts and must be run explicitly:

```powershell
cargo test --test providers_live -- --ignored --nocapture
cargo test --test live_artifacts -- --ignored --nocapture --test-threads=1
cargo test --test live_install -- --ignored --nocapture --test-threads=1
```

## Pull requests

- Keep one logical change per pull request.
- Add regression tests for parsers, installation transactions, path handling, and security boundaries.
- Update `AI_LOG.md` in the same development change.
- Do not commit downloaded browsers, generated EXEs, caches, secrets, or personal absolute paths.
- Preserve the MIT OR Apache-2.0 licensing model and update `THIRD_PARTY_NOTICES.md` for new runtime components.

## Browser support claims

Do not remove an experimental label based only on compile or extraction success. Require real-browser data placement, Cookie/login retention across restart, upgrade retention, and clean uninstall/cleanup evidence.
