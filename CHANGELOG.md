# Changelog

[简体中文](CHANGELOG.zh-CN.md) · English

## v0.1.0 — 2026-10-26

First stable release of Browser Harbor.

### Added

- One portable Windows x64 EXE for 14 Chromium- and Firefox-family browser targets.
- Verified downloads, controlled archive extraction, staged installation, atomic replacement, and rollback.
- Separate Browser Root, program, data, profile, and optional cache paths.
- Full Chrome++ integration and required Libportable integration for supported targets.
- Read-only configuration checks, safe runtime path repair, relocation preview, and browser identity validation.
- Immediate autosave, system/light/dark appearance, fixed compact UI, saved window position, and single-instance protection.
- Chinese and English interface switching with persisted language preference.
- Automatic Edge version-directory resolution.
- Chinese and English user, security, privacy, contribution, and release documentation.

### Supported targets

- Chromium: Brave, Catsxp, Chrome, Edge, Helium, Thorium, Turbo, Ungoogled Chromium.
- Firefox: Firefox, Floorp, Iceweasel, LibreWolf, SkyKakapo, Zen.

### Known limitations

- Brave, Helium, and Ungoogled Chromium retain experimental Chrome++ compatibility labels.
- The executable is not Authenticode-signed.
- Browser Root relocation requires the explicit runtime INI repair action and is not automatic.
- Existing-directory browser detection and import are not implemented.
