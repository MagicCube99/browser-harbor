<p align="center">
  <img src="assets/app-icon.png" width="112" height="112" alt="Browser Harbor app icon">
</p>

<h1 align="center">Browser Harbor</h1>

<p align="center">
  <b>Portable browser downloader and manager for Windows</b><br>
  Download, verify, install, update, and configure supported browsers without installing a browser manager.
</p>

<p align="center">
  <a href="https://github.com/MagicCube99/browser-harbor/releases/latest"><img src="https://img.shields.io/github/v/release/MagicCube99/browser-harbor?label=Download" alt="Download"></a>
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4" alt="Windows 10 | 11">
  <img src="https://img.shields.io/badge/x64-64--bit-blue" alt="x64">
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green" alt="MIT OR Apache-2.0"></a>
</p>

<p align="center">
  <a href="README.zh-CN.md">简体中文</a> ·
  <a href="docs/USER_GUIDE.md">User guide</a> ·
  <a href="CHANGELOG.md">Changelog</a>
</p>

Browser Harbor is a compact, portable Windows utility for 14 Chromium- and Firefox-family browser targets. It downloads from declared upstream sources, verifies artifacts, builds portable layouts, preserves user data, repairs runtime paths, and keeps the browser program, data, profiles, and cache separate.

It does not launch browsers, register itself as a browser, manage browser processes, create taskbar pins, or read browser history.

## Download

Download `BrowserHarbor-0.2.0-beta.1-x64.exe` from the [v0.2.0-beta.1 release](https://github.com/MagicCube99/browser-harbor/releases/tag/v0.2.0-beta.1).

Run the EXE from any writable folder. No installer, administrator rights, background service, or WebView2 runtime is required.

Requirements: Windows 10 22H2 or Windows 11, x64.

`v0.1.0` remains available as the frozen compatibility build. The `v0.2.x` Verified Edition uses signed release metadata; it does not change the existing browser-artifact mirror download path.

## Highlights

- One native EXE for 14 browser targets.
- Ed25519-verified release metadata, required artifact SHA-256/SHA-512, and LibreWolf OpenPGP verification.
- Controlled extraction, staged installation, atomic replacement, and rollback.
- Separate Browser Root, program, data, profile, and optional cache paths.
- Full Chrome++ integration and required Libportable integration where applicable.
- Read-only configuration checks, runtime INI repair, and browser identity validation.
- Automatic Edge version-directory resolution and single-instance protection.
- Immediate autosave, follow-system light/dark appearance, and a fixed compact window.
- Chinese and English interface switching, persisted across restarts.

## Supported browsers

| Family | Targets |
|---|---|
| Chromium | Brave, Catsxp, Chrome, Edge, Helium, Thorium, Turbo, Ungoogled Chromium |
| Firefox | Firefox, Floorp, Iceweasel, LibreWolf, SkyKakapo, Zen |

Brave, Helium, and Ungoogled Chromium retain an experimental Chrome++ compatibility label. Iceweasel provides ESR and Release channels; Thorium provides SSE3, SSE4, AVX, AVX2, and AVX512 builds.

## Safety

- Downloaded browser installers are never executed as setup programs.
- Official APIs and the project-signed manifest establish identity; mirrors and accelerators carry bytes only.
- The embedded signed manifest covers all 19 channel/CPU combinations and Chrome++/Libportable, so a blocked metadata endpoint does not remove the offline bootstrap path.
- A scheduled metadata workflow refreshes and signs upstream changes without publishing a new EXE.
- Program replacements preserve user data and runtime configuration according to target rules.
- The EXE is not Authenticode-signed yet, so Windows SmartScreen may show a warning.

See [SECURITY.md](SECURITY.md), [PRIVACY.md](PRIVACY.md), and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Documentation

- [User guide](docs/USER_GUIDE.md) / [中文用户指南](docs/USER_GUIDE.zh-CN.md)
- [Changelog](CHANGELOG.md) / [中文更新记录](CHANGELOG.zh-CN.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Contributing](CONTRIBUTING.md)

## Build from source

Prerequisites: Rust 1.99.1 and Visual Studio Build Tools with the C++ desktop workload.

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo build --release
```

Or run the complete Windows quality gate:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check.ps1
```

## License

Project source is available under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option (`MIT OR Apache-2.0`).

Third-party components and browser packages retain their own licenses. Browser Harbor is not affiliated with or endorsed by browser vendors.
