# Changelog

[简体中文](CHANGELOG.zh-CN.md) · English

## v0.4.0 — 2026-10-27

### Added

- Chrome++ can now be updated independently from the browser after an explicit update check.
- Browser upgrades show both browser and Chrome++ version transitions before proceeding.

### Reliability

- Chrome++ update preflight scans every process in the selected target’s program directory, including multiple windows, background processes, and Edge version directories.
- Running other browser brands do not block the selected target, and Browser Harbor never force-closes a browser.
- Chrome++ update stages and backs up files, checks for running instances again before commit, and rolls back on failure.
- One verified Chrome++ artifact is reused by later targets while its SHA-256 cache entry remains available.

## v0.3.1 — 2026-10-27

### Fixed

- The Chrome++ integration and configuration cards now use the same heading rhythm, with the installed Chrome++ version and update action moved into the integration heading row.
- Footer status messages now use all space before the right-side controls; long messages still truncate before the language, appearance, global settings, and version controls.

## v0.3.0 — 2026-10-27

### Changed

- Floorp and LibreWolf now use mandatory Libportable injection instead of package-bundled launchers.
- All Firefox-family targets now remain portable when their browser executable is launched directly.
- Floorp keeps `data/profile/default`; LibreWolf keeps `Profiles/Default`.
- Bundled Floorp/LibreWolf launchers and LibreWolf updater/task scripts are removed to prevent update and launch conflicts.
- Floorp's local version display strips the internal `@<Gecko version>` suffix, so it compares correctly with upstream.

### Reliability

- Existing custom browser and profile paths are preserved when upgrading from older settings.
- Portable setup is only reported as successful after runtime injection is verified.

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
