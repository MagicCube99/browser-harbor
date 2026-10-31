# Changelog

[简体中文](CHANGELOG.zh-CN.md) · English

## v0.6.10 — 2026-10-31

### Fixed

- Fixed Edge 156 Stable installation failing during archive extraction.
- Edge Stable and Beta now use Microsoft's official CDP 7z payload, which contains the complete `MSEDGE.7Z` browser archive.
- Removed obsolete Edge Enterprise MSI entries from signed metadata generation and fallback handling.

## v0.6.9 — 2026-10-31

### Added

- Added Vivaldi (Stable x64) as the 15th supported browser target.
- Pure in-memory double-layer 7z unpacking from the official installer without executing third-party installation binaries.
- Configured official native Standalone mode via `stp.viv`, keeping `User Data` strictly isolated in the browser root without modifying vendor binaries or touching registry.
- Automatically strips background `update_notifier.exe` on install and update.
- Updated documentation and architecture specifications across all public and internal references.

## v0.6.8 — 2026-10-30

### Changed

- Refined cross-channel upgrade experience: when the installed browser channel
  differs from the selected channel, the overview displays a clear "Channel mismatch"
  status and the action button clearly states "Install using {Channel} channel"
  (or "Install from local {Channel} package"), eliminating misleading version comparisons.
- Added channel detection for Iceweasel (ESR and Release).
- Forward-expanded Floorp Libportable compatibility gate to v12–v14.

## v0.6.7 — 2026-10-30

### Fixed

- Fixed Libportable version gate blocking Zen browser updates to 1.24b+.
- Improved runtime preparation error reporting with specific error descriptions
  and localization.

## v0.6.6 — 2026-10-29

### Fixed

- Fixed a configuration check false positive where upgraded legacy configs
  drifted between internal profile and user data paths until manually reselected.
- Added actionable guidance for Windows permission barriers when switching to
  portable configuration mode or installing under protected directories like
  C:\ or Program Files.

## v0.6.5 — 2026-10-29

### Changed

- “Load local package” is now “Choose local installer package,” and the
  follow-up action names no longer imply that the browser will be downloaded.
- Local-package selection now shows the selected target, channel, build, and
  path in the footer.
- Mismatched local packages now report the expected browser, version, and file
  name instead of a generic size or digest error.

## v0.6.4 — 2026-10-29

### Changed

- A newer Browser Harbor release now turns the footer version control into a
  highlighted update chip.
- Clicking the version control opens a compact release-site menu with GitHub
  and Gitee. No full URLs or imitation brand icons are shown.

## v0.6.3 — 2026-10-29

### Fixed

- Folder selection no longer hits Windows Shell’s “folder unavailable” error
  when a directory and an archive with the same base name exist side by side.
  The picker opens their parent instead.

## v0.6.2 — 2026-10-29

### Added

- Browser Harbor now checks the latest stable release at startup. When a newer
  version exists, the footer version link gains an `↑` and opens the release page.

### Fixed

- Native folder and file pickers now canonicalize their starting directory and
  fall back to the nearest existing parent, reducing transient Windows Shell
  “folder unavailable” errors.

## v0.6.1 — 2026-10-29

### Fixed

- Daily signed release metadata now refreshes reliably for browsers that require
  Browser Harbor to calculate their artifact hash, including Catsxp.
- If one browser source fails to refresh, Browser Harbor keeps that browser's
  previous signed entry and continues refreshing the rest of the manifest.
- The Browser Root example now names the currently selected browser instead of
  always showing Helium.

## v0.6.0 — 2026-10-29

### Added

- Chrome and Edge now offer Stable and Beta channels.
- Firefox now offers Stable, ESR, and Beta channels.
- Brave now offers Stable and Beta channels, selected by official release names instead of unreliable prerelease flags.
- Installation now detects the channel of the existing local executable and asks before replacing a different channel in the same Browser Root.

### Fixed

- Libportable now accepts tested Firefox 159 and 160 Beta builds while still rejecting untested 161 Nightly.
- The cache-directory input now shows a short “Leave empty to follow Profile” placeholder while retaining the full hover explanation.

## v0.5.4 — 2026-10-29

### Changed

- The manager configuration file is now named `BrowserHarbor.json` in both user and portable modes.
- Existing `manager.json` files are migrated automatically on startup. A conflicting older file is preserved as a numbered backup.

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
