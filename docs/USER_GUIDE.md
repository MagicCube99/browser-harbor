# Browser Harbor User Guide

[简体中文](USER_GUIDE.zh-CN.md) · English

## Requirements

- Windows 10 22H2 or Windows 11, x64.
- A writable directory for Browser Harbor and each browser's Browser Root.
- Network access to browser vendors, GitHub, and the selected runtime sources.

Browser Harbor is a portable single EXE. It does not require installation or administrator rights.

## First launch

1. Download `BrowserHarbor-0.1.0-x64.exe` from GitHub Releases.
2. Run the EXE from a writable folder you control.
3. Select a browser from the browser drop-down.
4. Review the Browser Root and child paths.
5. Use “检查配置” to inspect an existing installation before updating it.

Settings are saved immediately after each effective edit. There is no Save or Cancel button.

The manager configuration is stored at:

```text
%LOCALAPPDATA%\browser-harbor\manager.json
```

Use the `中` / `EN` control in the footer to switch the interface language. The choice is saved immediately.

## Browser pages

### Browser

- **Browser**: selects one of the 14 supported targets.
- **Channel**: available only when a target has multiple channels.
- **Build**: available for targets such as Thorium.
- **Current/upstream version**: read from the real browser and release metadata.
- **检查更新**: starts a background upstream metadata check.
- **下载安装/升级/重新下载并替换**: starts the verified installation workflow.
- **载入本地包**: uses an offline official package selected by the user.

Same-version replacement remains visible and requires confirmation. Browser Harbor never hides the action merely because versions match.

### Paths and data

Each target owns these paths:

- Browser Root
- Program directory
- Data directory
- Profile directory
- Optional cache directory

Changing Browser Root regenerates the standard child paths. You can override individual directories when the runtime supports it.

Directory fields accept `.\...` input, but saves resolve it against the EXE directory and store the absolute path in `manager.json`. Browser runtime files separately follow the same-volume-relative / cross-volume-absolute rule.

### Configuration check

“检查配置” is read-only. It checks:

- root, program, data, profile, and cache paths;
- the real x64 browser executable;
- local version availability;
- required portable runtime files;
- Edge `version.dll` import status;
- Chrome++, Libportable, Iceweasel, and SkyKakapo path configuration.

Statuses are **not installed**, **healthy**, **warning**, and **error**.

If `chrome++.ini` still contains absolute paths, diagnostics offers **Repair Chrome++ Paths**. After confirmation it verifies that the browser is closed, backs up the INI, and updates only `data_dir` and `cache_dir` while preserving all other options, comments, and unknown keys. The backup is `chrome++.ini.path-repair.bak`; collisions use `.2`, `.3`, and so on.

### Relocating a moved browser

1. Open “检查配置”.
2. Choose “重新定位…”.
3. Select the browser's new Browser Root.
4. Review the paths that will be applied.
5. Click “应用”.

Paths inside the old Browser Root are remapped. User-selected paths outside the old root are preserved. Relocation updates `manager.json`; it does not rewrite browser INI files.

## Chrome++

Chrome++ is supported for Brave, Chrome, Edge, Helium, Thorium, and Ungoogled Chromium.

- The Runtime is downloaded from the official Chrome++ release.
- Edge additionally uses the matching Chrome++ setdll helper to add the `version.dll` import.
- The configuration page exposes the full public Chrome++ 1.18.2 INI option set.
- Edits are written atomically to `chrome++.ini`.
- Restoring defaults creates a backup first.
- Brave, Helium, and Ungoogled Chromium remain experimental.

## Firefox-family runtimes

- Official Firefox and Zen require Libportable.
- Libportable injection failure is an installation failure.
- Iceweasel uses its package portable runtime.
- SkyKakapo uses its bundled `tmemutil` runtime.
- Floorp and LibreWolf keep their vendor portable layouts.

## Global settings

Open global settings from the bottom bar to configure:

- Built-in download accelerators.
- Additional custom HTTPS accelerator prefixes.
- A user proxy or the system proxy for final direct-GitHub fallback.

Accelerators only change transport. They cannot change version, size, hash, signature, or layout.

“清理下载缓存” removes only rebuildable `.artifact` and `.part` files. It does not remove browser data, profiles, or release metadata.

## Portable paths and moving directories

Browser Harbor prefers relocatable runtime paths when the runtime supports them:

- Chrome++: `%app%\..\...`
- Libportable and Iceweasel: paths relative to the final program directory
- SkyKakapo: `PortableDataPath=..\Profiles`

Cross-volume custom paths fall back to absolute paths and are reported as less portable.

These browser runtime settings do not depend on the manager EXE's volume. Even when the EXE is on `C:` and the browser is on `D:`, runtime files still use a relative expression whenever the browser program and data can be related on the same volume; the manager stores that Browser Root as an absolute path.

## Troubleshooting

### The browser is not detected

Run “检查配置”. Confirm the program directory contains the expected x64 executable and that the selected target is correct.

### A browser was moved

Use “重新定位” and select its new root. Do not edit unrelated browser targets.

### Chrome++ does not load

Check that `version.dll` and `chrome++.ini` are beside the browser executable. Edge additionally requires the import-table check shown in diagnostics.

### Firefox does not use the portable data directory

Confirm Libportable files exist and `portable.ini` contains the expected relative `PortableDataPath`.

### Windows warns about the EXE

The executable is not code-signed. Obtain it only from GitHub Releases and compare its SHA-256 with the value in the release notes.

## Uninstall

Delete the Browser Harbor EXE and `%LOCALAPPDATA%\browser-harbor\manager.json`. Browser installations and their user data remain in the Browser Roots you selected.

## Related documents

- [README](../README.md)
- [Changelog](../CHANGELOG.md)
- [Security](../SECURITY.md)
- [Privacy](../PRIVACY.md)
- [Third-party notices](../THIRD_PARTY_NOTICES.md)
