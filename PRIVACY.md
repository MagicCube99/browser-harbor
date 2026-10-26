# Privacy Notice

[简体中文](PRIVACY.zh-CN.md) · English

Browser Harbor is a local Windows utility. It has no telemetry, analytics, account system, advertising SDK, or remote configuration service.

## Local data

The application stores:

- manager settings in `%LOCALAPPDATA%\browser-harbor\manager.json`;
- rebuildable download cache in `%TEMP%\browser-harbor\cache`;
- browser program and user data only in the Browser Roots and overrides selected by the user.

Browser Harbor does not collect browser history, cookies, credentials, or profile contents for transmission.

## Network requests

Network access is used to:

- query browser, Chrome++, and Libportable release metadata;
- download browser packages and signed runtimes;
- reach GitHub and official vendor endpoints through selected transport routes.

Requests necessarily expose normal network metadata to those service providers, such as IP address, TLS connection data, requested URL, and user-agent. Browser Harbor does not add a persistent installation identifier to those requests.

System or user proxy settings may be used for final direct-GitHub fallback. Configured accelerators are contacted directly when enabled.

## Logs and diagnostics

Browser Harbor does not create a persistent application log. The configuration checker displays local paths and errors in the UI. Remove personal paths before posting diagnostics publicly.

## Data deletion

Delete `%LOCALAPPDATA%\browser-harbor\manager.json` to remove manager settings. Delete Browser Roots manually if you also want to remove browser programs and user data.
