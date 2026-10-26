# Changelog

[简体中文](CHANGELOG.zh-CN.md) · English

## v0.2.0 — 2026-10-26

First stable Verified Edition.

### Added

- Ed25519-signed release metadata with a public key embedded in the executable.
- Automatic signed-metadata refresh through GitHub Actions without requiring a new EXE or Release.
- One-session upstream checks and ten-minute verified release-descriptor reuse.

### Security

- Browser, Chrome++, Libportable, and local-package installation always require trusted integrity values.
- Accelerators may transport metadata and artifacts but cannot establish release identity.

### Compatibility

- `v0.1.0` remains frozen as the historical compatibility release.
- `manager.json` remains schema v10; trusted metadata is cached separately.

## v0.2.0-beta.3 — 2026-10-26

### Changed

- The browser page automatically checks the selected target/channel/build once per session.
- Installed browsers with unknown upstream state show **Check and install/upgrade**.
- Installations started within ten minutes reuse the matching verified release descriptor instead of resolving metadata again.
- Manual **Check for updates** remains available for refresh.

## v0.2.0-beta.2 — 2026-10-26

### Changed

- Signed release metadata no longer has an operational expiry.
- The client refreshes a valid signed manifest at most once per day instead of reusing it until an expiry date.
- The metadata workflow updates and signs the manifest only when upstream release information changes.

## v0.2.0-beta.1 — 2026-10-26

Verified Edition beta.

### Added

- Ed25519-signed release metadata with a public key embedded in the executable.
- A signed bootstrap manifest covering all 19 destination/channel/CPU combinations plus Chrome++ and Libportable.
- Signed-manifest refresh through verified mirrors, trusted cache fallback, and explicit metadata-source display.
- A maintenance tool for key generation, manifest signing, and verification.

### Security

- Official GitHub metadata no longer uses third-party accelerators.
- Browser, Chrome++, and Libportable installation now requires a non-empty integrity value.
- Local packages are subject to the same trusted digest verification.
- Removed Bush2021 metadata fallback from the normal Chrome/Edge resolution path.

### Compatibility

- `v0.1.0` remains frozen as the unsigned-metadata compatibility release.
- `manager.json` remains schema v10; trusted metadata is cached separately.

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
