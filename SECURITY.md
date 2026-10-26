# Security Policy

[简体中文](SECURITY.zh-CN.md) · English

## Supported versions

Security fixes are applied to the latest published release. Pre-release builds are supported for testing but should not be treated as a long-term stable channel.

## Reporting a vulnerability

Do not publish exploit details in a public issue.

Use GitHub's private vulnerability reporting / Security Advisory flow for this repository. Include:

- affected version and Windows build;
- browser target and runtime;
- reproduction steps;
- impact;
- relevant logs with personal paths removed;
- whether user data or downloaded binaries may be modified.

If private reporting is unavailable, open a minimal issue asking the maintainer for a private contact channel without disclosing the vulnerability.

## Trust boundaries

- Release metadata, redirects, archive names, file sizes, hashes, signatures, and archive paths are untrusted.
- Mirrors and accelerators may alter transport only. They must not alter the artifact identity.
- Browser installers are not executed.
- Chrome++, Libportable, browser packages, and launchers are downloaded from declared sources.
- Local packages must match trusted release information.
- The application does not request administrator privileges or modify system browser associations.

## Release verification

Only obtain binaries from GitHub Releases. Compare the downloaded EXE with the SHA-256 shown in the corresponding release notes.

The current executable is not Authenticode-signed. A valid SHA-256 proves the downloaded bytes match the published release; it does not provide identity or reputation like a code-signing certificate.

## Third-party vulnerabilities

Report vulnerabilities in Chrome++, Libportable, browsers, or launchers to their respective maintainers. Browser Harbor reports affected upstream components and avoids bundling those binaries in its source repository.
