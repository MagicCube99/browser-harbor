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
- Official APIs and the Ed25519-verified manifest establish release identity.
- Mirrors and accelerators may carry the signed manifest and artifact bytes only. They must not provide unsigned release identity.
- Browser, Chrome++, Libportable, and local-package installation paths require a non-empty trusted digest.
- Browser installers are not executed.
- Chrome++, Libportable, browser packages, and launchers are downloaded from declared sources.
- Local packages must match trusted release information.
- The application does not request administrator privileges or modify system browser associations.

## Release verification

Only obtain binaries from GitHub Releases or the project's linked Gitee mirror. Compare the downloaded EXE with the SHA-256 shown in the corresponding release notes. Both release locations provide `trusted-metadata.json`, `trusted-metadata.sig`, and `metadata-public-key.hex`.

The executable is not Authenticode-signed. The Ed25519 signature authenticates release metadata, not the EXE itself. A valid EXE SHA-256 proves the downloaded bytes match the published release; it does not provide identity or reputation like a code-signing certificate.

## Third-party vulnerabilities

Report vulnerabilities in Chrome++, Libportable, browsers, or launchers to their respective maintainers. Browser Harbor reports affected upstream components and avoids bundling those binaries in its source repository.
