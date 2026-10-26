Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Push-Location (Join-Path $PSScriptRoot "..")
try {
    & powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check-docs.ps1
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & cargo clippy --workspace --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & cargo test --workspace --all-targets
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & cargo build --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $exe = Join-Path (Get-Location) "target\release\browser-harbor.exe"
    $versionInfo = (Get-Item -LiteralPath $exe).VersionInfo
    $manifestVersion = [regex]::Match(
        (Get-Content -Raw -LiteralPath "Cargo.toml"),
        '(?m)^version\s*=\s*"([^"]+)"'
    ).Groups[1].Value
    if ($versionInfo.ProductVersion -ne $manifestVersion) {
        throw "Release executable version '$($versionInfo.ProductVersion)' does not match Cargo package version '$manifestVersion'."
    }
    if ($versionInfo.CompanyName -ne "MagicCube99") {
        throw "Release executable is missing the MagicCube99 build mark."
    }
    if ($versionInfo.FileDescription -ne "Browser Harbor" -or $versionInfo.ProductName -ne "Browser Harbor") {
        throw "Release executable product metadata is inconsistent."
    }
    if ($versionInfo.OriginalFilename -ne "browser-harbor.exe") {
        throw "Release executable original filename metadata is inconsistent."
    }
    if ($versionInfo.Comments -notmatch 'Built by MagicCube99') {
        throw "Release executable is missing the MagicCube99 author marker."
    }
    $expectedPrivateBuild = "MagicCube99/browser-harbor/$($versionInfo.ProductVersion)"
    if ($versionInfo.PrivateBuild -ne $expectedPrivateBuild) {
        throw "Release executable private-build marker is inconsistent: $($versionInfo.PrivateBuild)"
    }

    $stream = [System.IO.File]::OpenRead($exe)
    $reader = [System.IO.BinaryReader]::new($stream)
    try {
        if ($stream.Length -gt 8MB) {
            throw "Release executable exceeds the 8 MiB size budget: $($stream.Length) bytes."
        }
        if ($stream.Length -lt 512) { throw "Release executable is unexpectedly small." }
        $stream.Position = 0x3c
        $peOffset = $reader.ReadInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { throw "Release executable has no valid PE signature." }
        $stream.Position = $peOffset + 24 + 68
        $subsystem = $reader.ReadUInt16()
        if ($subsystem -ne 2) { throw "Release executable must use the Windows GUI subsystem (2), found $subsystem." }
    } finally {
        $reader.Dispose()
        $stream.Dispose()
    }
} finally {
    Pop-Location
}
