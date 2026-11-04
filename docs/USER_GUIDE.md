# Browser Harbor User Guide

[简体中文](USER_GUIDE.zh-CN.md) · English

## Requirements

- Windows 10 22H2 or Windows 11, x64.
- A writable folder for the Browser Harbor EXE.
- A writable Browser Root for each installed browser.
- Network access to the selected browser vendor or verified download source.

Browser Harbor is a portable single EXE. It does not require installation or administrator rights.

## First launch

1. Download the `BrowserHarbor-<version>-x64.exe` file from
   [GitHub Releases](https://github.com/MagicCube99/browser-harbor/releases/latest)
   or the [Gitee mirror](https://gitee.com/MagicCube99/browser-harbor/releases).
2. Run it from a writable folder.
3. Select a browser.
4. Review the program, data, profile, and cache paths.
5. Use **Check configuration** if you are connecting an existing portable installation.

Settings save immediately after each valid edit. There is no Save or Cancel button.

The manager configuration is stored at:

```text
%LOCALAPPDATA%\browser-harbor\BrowserHarbor.json
```

The global settings page can switch the manager configuration to portable mode,
which stores `BrowserHarbor.json` beside the Browser Harbor EXE. The EXE directory
must be writable. Browser paths inside the file are still absolute, so copying
the manager to another computer may require **Check configuration** and
**Relocate…**. Temporary directories are never used for persistent settings.
If an older `manager.json` is found, the next launch migrates it to
`BrowserHarbor.json`; conflicting files are preserved as numbered backups.

Use the `中` / `EN` control in the footer to switch languages. The choice is saved immediately.

At startup, Browser Harbor checks the latest stable release once. If a newer
version exists, the footer version control gains a highlighted `↑`. Click it
to choose GitHub or Gitee in a compact release-site menu. Browser Harbor does
not replace its own running EXE.

## Download and update

Open the browser page and choose one of these actions:

- **Check for updates**: refresh upstream release information.
- **Download and install/upgrade**: download, verify, and install the selected target.
- **Download again and replace**: replace the current program when the upstream version is the same.
- **Choose current release package**: use a local copy of the official release found by this check. Enable **Global settings → Advanced → Use a local package instead of downloading** first; it is off by default. The package must match the selected browser, channel, build, and checked version. Importing other versions is not supported. Size, digest, and required signatures are still verified, and a network connection may still be needed.

Browser Harbor checks the browser version and download integrity before changing the installation. Same-version replacement remains available but requires confirmation.

Downgrading also requires confirmation: an older browser may be unable to read user data migrated by a newer version. Back up your data before proceeding.

If the selected channel differs from the channel detected in the existing local executable, Browser Harbor asks before replacing the program directory.

The browser page checks the selected target, channel, and build once per session. A manual check is still available. An installation started within ten minutes can reuse the verified upstream result.

Selecting a local installer does not install it immediately. Use **Install from
local package** (or the corresponding reinstall/upgrade label) afterward.
Disabling local installer packages clears all saved selections, without deleting the package files.

If an installation was interrupted, start the app again and retry the selected browser. Browser Harbor restores unfinished file replacements before beginning a new attempt and reuses complete, verified downloads. Partial downloads can resume when the cache remains, the artifact has not changed, and the server supports it. The completed file must still pass full verification. The previous installation does not continue automatically. If files prevent recovery, the originals are retained and their location is shown; close the affected browser and retry.

## Directories

Catsxp offers Stable and Beta, with Stable selected by default. Beta revisions may share an EXE version; Browser Harbor checks installed program files before identifying the revision. Existing installations without a verified build record remain unconfirmed until reinstalled through Browser Harbor. Changing channels replaces the selected installation; it does not create a second installation. Back up your user data before switching from Beta to Stable.

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

Input such as `.\Helium` is resolved from the Browser Harbor EXE folder. `BrowserHarbor.json` stores resolved absolute paths. Browser runtime files use relative paths when program and data are on the same volume.

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

## Native Standalone portability (Vivaldi)

Vivaldi uses the official native Standalone portable mode:

- The official installer is extracted in two controlled stages into `App/`, without running the installer.
- The manager enables Vivaldi's native Standalone mode, with user data stored by default in `User Data/` beside `App/` under the Browser Root.
- Automatically removes the background update notifier (`update_notifier.exe`) during installation and upgrade.
- Vivaldi and Turbo default to `User Data/`; Catsxp defaults to `Profiles/`. These targets manage profiles within their user-data directory, so the app hides the duplicate Profile field.
- **Moving to another PC:** Vivaldi uses native Standalone mode without Chrome++. Vivaldi documents an official limitation: passwords and extensions are encrypted with a key tied to the current computer's user profile and cannot be moved just by copying the folder. Enable Vivaldi Sync beforehand to sync saved logins and extensions. Websites and extensions may still require a fresh sign-in; copying the folder does not guarantee that every signed-in session will transfer. See the [official Standalone documentation](https://help.vivaldi.com/desktop/install-update/standalone-version-of-vivaldi/).

When Vivaldi is selected, a short migration notice appears below **Paths and user data**. Hover over it for the full explanation. Other browsers do not show this notice.

## Global settings

The footer settings page controls built-in and custom download accelerators, proxy and timeout behavior, appearance, and download-cache cleanup.

Version checks use shorter timeouts. Downloads probe accelerators and prefer a route that promptly returns data; unresponsive or stalled accelerators fail over sooner. Original download URLs retain the timeout selected in settings. Every source must pass the same integrity checks; speed still depends on your network.

Accelerators only change transport. **Clear download cache** removes only reproducible installation files and never removes browser profiles or installed programs.

Signed verification info tries the project-maintained Gitee mirror first, then falls back to GitHub accelerator routes. The mirror changes transport only; signature, size, and digest checks still run locally, and an older mirror snapshot cannot replace a newer verified cache.

**Advanced** enables **Automatically check verification info** and **Use the latest signed offline verification info** by default. The first controls automatic checks when opening a browser page. The second permits the last verified cache only when live verification info is unavailable. The cache must still pass signature, size, and digest checks and cannot make a newer release installable without a trusted digest.

**Refresh verification info now** forcibly fetches and verifies the signed manifest without reinstalling a browser. Success clears previous upstream-check results; failure leaves the existing cache intact. When automatic checks are enabled, returning to the browser page checks again with the new manifest. This area also contains the local installer-package switch, which is off by default.

## Troubleshooting

### Manager settings are damaged

Use **Back up and reset config**, confirm the reset, and keep the backup for reference. This resets Browser Harbor settings only; installed browsers and their user data are not removed. **Copy diagnostics** is also available; remove personal paths before sharing them.

### Chrome++ configuration could not be applied

The pending edit is retained. Close software holding the INI file open, check write permissions, and use **Retry applying Chrome++ settings**. The manager only treats the edit as saved after both configuration files have been updated.

### The app does not open through Remote Desktop

Some graphics-driver and remote-session combinations may prevent the window from opening. Try running the same EXE on the local desktop. Browser Harbor does not change display drivers or Remote Desktop settings.

### The browser is not detected

Run **Check configuration**. Verify the target, Browser Root, program directory, and x64 browser executable.

### A browser was moved

Use **Relocate…** and select its new Browser Root. If the program identity cannot be confirmed, verify the directory again before applying.

### Chrome++ does not load

Confirm that `version.dll` and `chrome++.ini` are beside the browser executable. Edge also requires the import-table check shown in diagnostics.

### Windows warns about the EXE

The executable is not Authenticode-signed. Obtain it only from GitHub Releases or the linked Gitee mirror, then compare its SHA-256 with the release notes.

## Uninstall

Delete the Browser Harbor EXE and the active `BrowserHarbor.json` (either under
`%LOCALAPPDATA%\browser-harbor` or beside the EXE). Browser programs and user
data remain in their Browser Roots. You can also remove
`%TEMP%\browser-harbor\cache` and shortcuts created in Browser Roots.

## Related documents

- [README](../README.md)
- [Changelog](../CHANGELOG.md)
- [Security](../SECURITY.md)
- [Privacy](../PRIVACY.md)
- [Third-party notices](../THIRD_PARTY_NOTICES.md)
