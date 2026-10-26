param(
    [Parameter(Mandatory = $true)]
    [string]$Tag
)

$ErrorActionPreference = "Stop"

if ($Tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$') {
    throw "Tag '$Tag' is not a supported release version."
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$version = $Tag.TrimStart("v")
$manifestVersion = [regex]::Match(
    (Get-Content -Raw -LiteralPath (Join-Path $repoRoot "Cargo.toml")),
    '(?m)^version\s*=\s*"([^"]+)"'
).Groups[1].Value
if ($manifestVersion -ne $version) {
    throw "Tag version '$version' does not match Cargo.toml version '$manifestVersion'."
}
$executable = Join-Path $repoRoot "target\release\browser-harbor.exe"
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "Release executable is missing. Run scripts/check.ps1 first."
}
$dist = Join-Path $repoRoot "dist"
if (Test-Path -LiteralPath $dist) {
    $resolvedDist = (Resolve-Path -LiteralPath $dist).Path
    if (-not $resolvedDist.StartsWith($repoRoot + [System.IO.Path]::DirectorySeparatorChar)) {
        throw "Refusing to remove a dist directory outside the repository."
    }
    Remove-Item -LiteralPath $resolvedDist -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null

$asset = Join-Path $dist "BrowserHarbor-$version-x64.exe"
Copy-Item -LiteralPath $executable -Destination $asset
$metadata = Join-Path $repoRoot "assets\trusted-metadata.json"
$signature = Join-Path $repoRoot "assets\trusted-metadata.sig"
$publicKey = Join-Path $repoRoot "assets\metadata-public-key.hex"
& cargo run --quiet --manifest-path (Join-Path $repoRoot "Cargo.toml") --bin metadata -- verify --manifest $metadata --signature $signature --public-key $publicKey
if ($LASTEXITCODE -ne 0) {
    throw "Trusted metadata signature verification failed."
}
Copy-Item -LiteralPath $metadata -Destination (Join-Path $dist "trusted-metadata.json")
Copy-Item -LiteralPath $signature -Destination (Join-Path $dist "trusted-metadata.sig")
Copy-Item -LiteralPath $publicKey -Destination (Join-Path $dist "metadata-public-key.hex")
$hash = (Get-FileHash -LiteralPath $asset -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "Packaged release asset: $asset"
Write-Host "SHA-256: $hash"
