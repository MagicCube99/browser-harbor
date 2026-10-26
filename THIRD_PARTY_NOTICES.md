# Third-Party Notices

This project downloads third-party runtime components and browser distributions from their official sources. It does not vendor Chrome++, setdll, Libportable, or browser binaries in its source tree.

## Chrome++ Next

- Component: Chrome++ Next 1.18.x
- Project: <https://github.com/Bush2021/chrome_plus>
- License: GPL-3.0-only
- Use: downloaded at runtime and installed beside supported Chromium executables
- Integration boundary: the project does not copy Chrome++ source into its own codebase

The application must display the component version, source repository, and license link wherever installation or update is offered. If a future release bundles, mirrors, or redistributes Chrome++ instead of downloading it directly from the upstream release, the distribution must also provide the applicable GPL license text and corresponding source in compliance with GPL-3.0.

## Chrome++ setdll

- Component: Chrome++ setdll 1.18.x
- Project: <https://github.com/Bush2021/chrome_plus>
- License: GPL-3.0-only
- Use: downloaded at runtime for Microsoft Edge only, then used in the staging directory to add a `version.dll` import to `msedge.exe`
- Integration boundary: the project does not copy setdll or its source into this codebase

setdll is obtained from the same verified GitHub Release as Chrome++. It modifies only the staged Edge executable; installation fails unless the resulting PE import table contains `version.dll`.

## Libportable

- Component: Libportable 9.0.x
- Project: <https://github.com/adonais/libportable>
- License: BSD-3-Clause
- Use: downloaded at runtime and injected into supported Firefox-family browser payloads

```text
Copyright (c) 2013-2025, libportable for Iceweasel project authors. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are
permitted provided that the following conditions are met:

   1. Redistributions of source code must retain the above copyright notice, this list of
      conditions and the following disclaimer.

   2. Redistributions in binary form must reproduce the above copyright notice, this list
      of conditions and the following disclaimer in the documentation and/or other materials
      provided with the distribution.

THIS SOFTWARE IS PROVIDED BY LIBPORTABLE PROJECT AUTHORS ``AS IS'' AND ANY EXPRESS OR IMPLIED
WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL LIBPORTABLE PROJECT AUTHORS OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## LibreWolf Public Signing Key

- Key: LibreWolf Maintainers `<gpg@librewolf.net>`
- Fingerprint: `662E3CDD6FE329002D0CA5BB40339DD82B12EF16`
- Source: Ubuntu Keyserver lookup for signing key ID `915585A1C36690B1` on 2026-10-24
- Use: embedded only to verify detached OpenPGP signatures on the official Windows portable ZIP

The public key is not redistributable software and grants no signing capability.

## Direct Rust Dependencies

Licenses below were read from the exact packages resolved in `Cargo.lock`; transitive dependencies retain their own licenses.

- GUI and UI tests: eframe, egui and egui_kittest 0.37.0 (`MIT OR Apache-2.0`).
- External browser links: webbrowser 1.2.5 (`MIT OR Apache-2.0`).
- Native directory selection: rfd 0.17.2 (`MIT`), using the Windows `IFileOpenDialog` folder picker without Linux backend features.
- Windows icon resources: winresource 0.1.34 (`MIT`), build-time only.
- Networking and runtime: reqwest 0.13.5 (`MIT OR Apache-2.0`), tokio 1.54.1 (`MIT`), futures-util 0.3.34 (`MIT OR Apache-2.0`).
- Archives: zip 9.0.5 (`MIT`), sevenz-rust2 0.24.1 (`Apache-2.0`, built with LZMA/LZMA2/BCJ support but without BZIP2/PPMd), msi 0.10 and cab 0.6 (`MIT`), quick-xml (`MIT`).
- Integrity and signatures: sha2 (`MIT OR Apache-2.0`), pgp 0.21 (`MIT OR Apache-2.0`), ed25519-dalek 2.2 (`BSD-3-Clause`) and rand_core 0.6 (`MIT OR Apache-2.0`).
- Data, parsing and platform: serde, serde_json, thiserror, regex-lite, url, base64 and windows-sys (`MIT OR Apache-2.0`); walkdir (`Unlicense/MIT`).
- Development-only: tempfile and egui_system_font_provider; these are not included in the release binary.

## Rust and Build Dependencies

Rust crates are consumed through Cargo and are not copied into the project source tree. The direct-dependency list above is a review aid, not a replacement for a generated transitive notice if the binary is redistributed outside the source repository.

Do not generate this list from `Cargo.toml` alone; it must reflect the resolved dependency graph actually shipped in the binary.

## Browser Distributions

Browser packages, launchers, and bundled runtimes downloaded by the manager are not redistributed with this project. Each package retains its own license, notices, and signatures. The manager must not remove license files from extracted browser packages.

Floorp Portable uses a Portapps-based launcher, and LibreWolf uses its official portable package. Their respective package licenses and notices remain in force for those downloaded artifacts.

## Application Icon

`assets/app-icon.png` and `assets/app-icon.ico` were derived from artwork supplied by the project owner for use as this application's icon. They are distributed under the same `MIT OR Apache-2.0` terms as the project unless the owner states otherwise.

## System Fonts

The application renders with fonts installed by Windows and does not redistribute Segoe UI, Microsoft YaHei, Consolas, or other system font files.

## Maintenance

Update this file in the same change that adds, removes, mirrors, or bundles a third-party runtime component. License review is required if the project starts redistributing Chrome++, Libportable, browser binaries, or their launchers.
