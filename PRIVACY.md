# Privacy Notice

[简体中文](PRIVACY.zh-CN.md) · English

Browser Harbor is a local Windows utility. It has no telemetry, analytics, account system, advertising SDK, or remote user-configuration service.

## Local data

The application stores:

- manager settings in `%LOCALAPPDATA%\browser-harbor\BrowserHarbor.json` by default, or beside the EXE when portable configuration storage is selected;
- rebuildable download cache in `%TEMP%\browser-harbor\cache`;
- browser program and user data only in the Browser Roots and overrides selected by the user.

Browser Harbor does not collect browser history, cookies, credentials, or profile contents for transmission.

Vivaldi Sync is an optional service of the browser itself. Browser Harbor only displays migration advice; it does not enable Sync, sign in to a browser account, or upload browser data.

## Network requests

Network access is used to:

- query browser, Chrome++, and Libportable release metadata;
- refresh the Ed25519-signed Browser Harbor release manifest through direct or accelerated transport;
- download browser packages and signed runtimes;
- reach GitHub and official vendor endpoints through selected transport routes.

Requests necessarily expose normal network metadata to those service providers, such as IP address, TLS connection data, requested URL, and user-agent. Browser Harbor does not add a persistent installation identifier to those requests.

Official metadata uses direct or user-proxy transport only. The signed manifest is cached under the rebuildable download cache and may be fetched through configured accelerators when enabled.

## Logs and diagnostics

Browser Harbor does not create a persistent application log. The configuration checker displays local paths and errors in the UI. Remove personal paths before posting diagnostics publicly. Cached signed metadata contains release information only, not browser user data.

## Data deletion

Delete the active `BrowserHarbor.json` shown under **Configuration location** to remove manager settings. If you previously used both storage locations, inactive copies may remain in the other location. Delete Browser Roots manually if you also want to remove browser programs and user data.
