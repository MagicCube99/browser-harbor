# Changelog

[简体中文](CHANGELOG.zh-CN.md) · English

## v0.3.0 — 2026-10-27

### Changed

- Floorp and LibreWolf now use mandatory Libportable injection instead of package-bundled launchers.
- All Firefox-family targets now remain portable when their browser executable is launched directly.
- Floorp keeps `data/profile/default`; LibreWolf keeps `Profiles/Default`.
- Bundled Floorp/LibreWolf launchers and LibreWolf updater/task scripts are removed to prevent update and launch conflicts.

### Reliability

- Configuration schema v11 migrates only known old default profile paths and preserves custom paths.
- Libportable installation now verifies that `mozglue.dll` imports `portable64.dll`.

## v0.2.1 — 2026-10-26

### Fixed

- Browser relocation now rejects a known different browser even when both use the same `chrome.exe` filename. Unknown custom programs require an explicit identity warning during confirmation.
- A saved window position is clamped to the current virtual desktop, preventing the application from opening entirely off-screen after a display change.

### Added

- The Chrome++ page now links to its upstream source and GPL-3.0 license.

### Documentation

- Public documentation now focuses on installation, operation, verification, and troubleshooting.

## v0.2.0 — 2026-10-26

- First stable release with verified downloads.
- Signed release metadata, automatic upstream checks, and verified-result reuse.
- Required integrity verification for browsers, Chrome++, Libportable, and local packages.
- Chinese and English user interface.

## v0.1.0 — 2026-10-26

- Initial public release with 14 browser targets, portable layouts, Chrome++ and Libportable integration, configuration checks, and immediate autosave.

## Current limitations

- Brave, Helium, and Ungoogled Chromium retain an experimental Chrome++ compatibility label.
- The executable is not Authenticode-signed.
- Existing arbitrary browser directories are not imported automatically.
- Default-browser registration, taskbar integration, browser launching, process supervision, and history access are intentionally out of scope.
