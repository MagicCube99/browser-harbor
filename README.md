<p align="center">
  <img src="assets/app-icon.png" width="112" height="112" alt="Browser Harbor app icon">
</p>

<h1 align="center">Browser Harbor</h1>

<p align="center">
  <b>Portable browser downloader and manager for Windows</b><br>
  Download, verify, install, update, and configure 14 browser targets.
</p>

<p align="center">
  <a href="https://github.com/MagicCube99/browser-harbor/releases"><img src="https://img.shields.io/badge/Download-Releases-blue" alt="Download releases"></a>
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4" alt="Windows 10 | 11">
  <img src="https://img.shields.io/badge/x64-64--bit-blue" alt="x64">
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green" alt="MIT OR Apache-2.0"></a>
</p>

<p align="center">
  <a href="README.zh-CN.md">简体中文</a> ·
  <a href="docs/USER_GUIDE.md">User guide</a> ·
  <a href="CHANGELOG.md">Changelog</a>
</p>

Browser Harbor is a small native Windows utility that downloads browser packages, verifies them, builds portable layouts, and preserves user data during updates. It manages program and data directories separately, so profiles remain under your control.

It does not launch browsers, register a default browser, create taskbar pins, manage browser processes, or read browser history.

## Download

Download `BrowserHarbor-0.2.1-x64.exe` from [GitHub Releases](https://github.com/MagicCube99/browser-harbor/releases/latest).

Run the EXE from any writable folder. No installer, administrator rights, background service, or WebView2 runtime is required.

Requirements: Windows 10 22H2 or Windows 11, x64.

The EXE is sufficient for normal use. The separately attached manifest, signature, and public key are optional materials for independent verification.

## Features

- Download, verify, install, update, and relocate supported portable browsers.
- Keep browser program, data, profile, and optional cache directories separate.
- Full Chrome++ integration for supported Chromium targets and required Libportable integration for official Firefox and Zen.
- Read-only configuration checks and safe path repair.
- Automatic shortcut creation in each Browser Root.
- Immediate autosave, system/light/dark appearance, and Chinese/English UI.

## Supported browsers

| Family | Browsers |
|---|---|
| Chromium | Brave, Catsxp, Chrome, Edge, Helium, Thorium, Turbo, Ungoogled Chromium |
| Firefox | Firefox, Floorp, Iceweasel, LibreWolf, SkyKakapo, Zen |

Iceweasel provides ESR and Release channels. Thorium provides SSE3, SSE4, AVX, AVX2, and AVX512 builds.

## Current limitations

- Brave, Helium, and Ungoogled Chromium retain an experimental Chrome++ compatibility label.
- The EXE is not Authenticode-signed, so Windows SmartScreen may show a warning.
- Existing arbitrary browser directories are not imported automatically; select the target and use “Check configuration” or “Relocate…” explicitly.
- Default-browser registration, taskbar integration, browser launching, process supervision, and history access are intentionally out of scope.

## Verify a download

Compare the EXE with the SHA-256 in its release notes. Browser packages and runtime components are checked before installation; mirrors only transport bytes and cannot change the expected version, hash, signature, or layout.

## Documentation

- [User guide](docs/USER_GUIDE.md) / [中文用户指南](docs/USER_GUIDE.zh-CN.md)
- [Changelog](CHANGELOG.md) / [中文更新记录](CHANGELOG.zh-CN.md)
- [Security](SECURITY.md) / [Privacy](PRIVACY.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
- [Contributing](CONTRIBUTING.md)

## License

Project source is available under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option (`MIT OR Apache-2.0`).

Third-party components and browser packages retain their own licenses. Browser Harbor is not affiliated with or endorsed by browser vendors.
