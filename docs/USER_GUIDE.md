# Browser Harbor User Guide

[简体中文](USER_GUIDE.zh-CN.md) · English

## Requirements

- Windows 10 22H2 or Windows 11, x64.
- A writable folder for the Browser Harbor EXE.
- A writable Browser Root for each installed browser.
- Network access to the selected browser vendor or verified download source.

Browser Harbor is a portable single EXE. It does not require installation or administrator rights.

## First launch

1. Download `BrowserHarbor-0.5.0-x64.exe` from
   [GitHub Releases](https://github.com/MagicCube99/browser-harbor/releases/latest)
   or the [Gitee mirror](https://gitee.com/MagicCube99/browser-harbor/releases).
2. Run it from a writable folder.
3. Select a browser.
4. Review the program, data, profile, and cache paths.
5. Use **Check configuration** if you are connecting an existing portable installation.

Settings save immediately after each valid edit. There is no Save or Cancel button.

The manager configuration is stored at:

```text
%LOCALAPPDATA%\browser-harbor\manager.json
```

The global settings page can switch the manager configuration to portable mode,
which stores `manager.json` beside the Browser Harbor EXE. The EXE directory
must be writable. Browser paths inside the file are still absolute, so copying
the manager to another computer may require **Check configuration** and
**Relocate…**. Temporary directories are never used for persistent settings.

Use the `中` / `EN` control in the footer to switch languages. The choice is saved immediately.

## Download and update

Open the browser page and choose one of these actions:

- **Check for updates**: refresh upstream release information.
- **Download and install/upgrade**: download, verify, and install the selected target.
- **Download again and replace**: replace the current program when the upstream version is the same.
- **Load local package**: use an official package you already downloaded.

Browser Harbor checks the browser version and download integrity before changing the installation. Same-version replacement remains available but requires confirmation.

The browser page checks the selected target, channel, and build once per session. A manual check is still available. An installation started within ten minutes can reuse the verified upstream result.

## Directories

Each Browser Target has its own:

- Browser Root
- Program directory
- User data directory
- Profile directory
- Optional cache directory

The user data directory is the root for bookmarks, history, cookies, sign-in
state, and browser settings. Most targets manage their internal profiles
automatically. A separate **Profile location** field appears only for Floorp
and LibreWolf, whose distributions need an explicit profile location.

The cache field is a cache **root**, not necessarily the final directory. A
Chromium target commonly places HTTP cache files under
`<cache root>\<Profile>\Cache\Cache_Data`. Code cache, GPU cache, and other
browser-managed caches may still remain inside the user data directory.

Changing Browser Root recalculates the standard child directories. Individual directories can be overridden when the browser runtime supports it.

After changing User data, Profile, or Cache for an already installed browser, close
the browser and run **Check configuration** so the portable runtime INI can be
updated. Editing the manager fields alone does not rewrite a running browser.

Input such as `.\Helium` is resolved from the Browser Harbor EXE folder. `manager.json` stores resolved absolute paths. Browser runtime files use relative paths when program and data are on the same volume.

## Check and repair

**Check configuration** is read-only. It checks:

- required directories and the real x64 browser executable;
- browser identity when detectable;
- local version availability;
- required portable runtime files;
- Edge `version.dll` import status;
- Chrome++, Libportable, Iceweasel, and SkyKakapo path settings.

If Chrome++ contains stale absolute paths, diagnostics can repair them after confirmation. The browser must be closed and the current INI is backed up first.

### Relocate a moved browser

1. Open **Check configuration**.
2. Choose **Relocate…**.
3. Select the new Browser Root.
4. Review the paths and any identity warning.
5. Choose **Apply**.

Paths inside the old Browser Root are remapped. Explicit paths outside it are preserved. Relocation updates only the manager configuration; it does not rewrite browser INI files.

## Chrome++

Chrome++ is available for Brave, Chrome, Edge, Helium, Thorium, and Ungoogled Chromium. The Chrome++ page shows the installed version, the integrated source/license link, and the complete public configuration set.

Browser installation or upgrade downloads, verifies, and installs Chrome++
automatically; users do not need to download `version.dll` or
`chrome++.ini` manually. Chrome++ settings are stored in `chrome++.ini` beside
the browser executable. The browser’s own preferences are stored in its
portable user-data profile, not in the Windows registry.

The refresh control only checks upstream. When a newer version is available it becomes an update control. Updating Chrome++ replaces only Chrome++ files, preserves browser data and profile settings, and backs up the previous INI. Only the selected target’s program directory is checked: other running browser brands do not block it. If any instance of the selected target is running, close every instance manually; Browser Harbor never terminates it.

Chrome++ release artifacts are cached by SHA-256 under `%TEMP%\browser-harbor\cache`. Updating another target with the same version reuses the verified archive without downloading it again.

Edge uses the matching `setdll` helper to add the `version.dll` import. Brave, Helium, and Ungoogled Chromium remain experimental.

## Firefox-family runtimes

- Firefox, Floorp, LibreWolf, and Zen require Libportable.
- Iceweasel uses its package portable runtime.
- SkyKakapo uses its bundled `tmemutil` runtime.
- Floorp and LibreWolf are made portable at the browser executable itself; Browser Harbor removes their bundled launchers and LibreWolf's updater/task scripts during installation or update.

## Global settings

The footer settings page controls built-in and custom download accelerators, proxy and timeout behavior, appearance, and download-cache cleanup.

Accelerators only change transport. **Clear download cache** removes only reproducible installation files and never removes browser profiles or installed programs.

## Troubleshooting

### The browser is not detected

Run **Check configuration**. Verify the target, Browser Root, program directory, and x64 browser executable.

### A browser was moved

Use **Relocate…** and select its new Browser Root. If the program identity cannot be confirmed, verify the directory again before applying.

### Chrome++ does not load

Confirm that `version.dll` and `chrome++.ini` are beside the browser executable. Edge also requires the import-table check shown in diagnostics.

### Windows warns about the EXE

The executable is not Authenticode-signed. Obtain it only from GitHub Releases or the linked Gitee mirror, then compare its SHA-256 with the release notes.

## Uninstall

Delete the Browser Harbor EXE and the active `manager.json` (either under
`%LOCALAPPDATA%\browser-harbor` or beside the EXE). Browser programs and user
data remain in their Browser Roots. You can also remove
`%TEMP%\browser-harbor\cache` and shortcuts created in Browser Roots.

## Related documents

- [README](../README.md)
- [Changelog](../CHANGELOG.md)
- [Security](../SECURITY.md)
- [Privacy](../PRIVACY.md)
- [Third-party notices](../THIRD_PARTY_NOTICES.md)
