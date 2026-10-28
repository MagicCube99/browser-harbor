# Changelog

[简体中文](CHANGELOG.zh-CN.md) · English

## v0.5.3 — 2026-10-28

### Changed

- Rewrote the cache-directory hint in plain language without exposing a raw path template.
- The configuration dialog now uses the same “Check configuration” name as the button that opens it.

## v0.5.2 — 2026-10-28

### Changed

- The timeout and proxy inputs now share one left edge in global settings.
- The current configuration path uses the full remaining row width and remains visible without a narrow side column.
- Proxy and cache cards now keep equal height; the cache cleanup explanation is available on hover.
- “HTTPS prefix” is renamed to “Accelerator prefix” so its purpose is clear without transport jargon.
- English global settings now fit without scrolling, with complete localized labels for the configuration path.

## v0.5.1 — 2026-10-28

### Changed

- Proxy/timeout settings and download-cache controls now share one two-column row in global settings.
- This reduces vertical scrolling and brings Configuration location into view sooner.

## v0.5.0 — 2026-10-28

### Added

- `manager.json` can now live in the per-user configuration directory or beside the Browser Harbor EXE for portable use.
- Switching configuration locations writes the current settings to the selected location; portable mode requires a writable EXE directory.

### Changed

- Most targets now expose one **User data directory** instead of separate User data and Profile fields. The interface keeps the internal Profile path synchronized automatically.
- The separate **Profile location** field is shown only for Floorp and LibreWolf, where the distribution needs an explicit profile location.
- Configuration diagnostics and relocation previews follow the same visibility rule, avoiding duplicate Profile rows for targets that share one data root.

### Fixed

- “Data directory” and “Profile directory” no longer appear as two equivalent editable fields for browsers that use the same root.

## v0.4.2 — 2026-10-28

### Changed

- Renamed the ambiguous “Data directory” field to “User data directory” and added explanations to every directory label.
- Clarified that Profile is an advanced per-profile subdirectory and that clearing a required derived path restores its default.
- Clarified that the custom cache field is a cache root; Chromium commonly stores HTTP cache under `<cache root>\<Profile>\Cache\Cache_Data`.
- Documented that path changes for an existing browser are applied through **Check configuration** and its portable INI repair.

### Fixed

- The Chrome++ page now explains that browser installation or upgrade downloads and installs Chrome++ automatically.
- The Chrome++ update tooltip now states that a detected version changes only after the confirmed replacement finishes.

## v0.4.1 — 2026-10-28

### Fixed

- A deleted Browser Root or Program Directory is now shown as “Directory missing” instead of reusing the last-install receipt as the current version.
- An existing Program Directory without the expected browser executable is shown as “Installation incomplete”.
- Install paths and the last-install receipt remain preserved, so the user can reinstall in place or reconnect removable storage.
- An uninstalled Chrome++ runtime no longer displays its previous stored version as the current version.
- Truncated footer and Chrome++ version labels show only one tooltip on hover.
- The shortcut toggle now shares the same right edge as the directory fields.

## v0.4.0 — 2026-10-27

### Added

- Chrome++ can now be updated independently from the browser after an explicit update check.
- When a Chrome++ update has been detected, browser upgrades show both browser and Chrome++ version transitions before proceeding.

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
