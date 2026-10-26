param(
    [Parameter(Mandatory = $true)]
    [string]$Tag
)

$ErrorActionPreference = "Stop"

if ($Tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?$') {
    throw "Tag '$Tag' is not a supported release version."
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$dist = Join-Path $repoRoot "dist"
if (Test-Path -LiteralPath $dist) {
    $resolvedDist = (Resolve-Path -LiteralPath $dist).Path
    if (-not $resolvedDist.StartsWith($repoRoot + [System.IO.Path]::DirectorySeparatorChar)) {
        throw "Refusing to remove a dist directory outside the repository."
    }
    Remove-Item -LiteralPath $resolvedDist -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null

$executable = Join-Path $repoRoot "target\release\browser-harbor.exe"
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "Release executable is missing. Run scripts/check.ps1 first."
}
$version = $Tag.TrimStart("v")
$asset = Join-Path $dist "BrowserHarbor-$version-x64.exe"
Copy-Item -LiteralPath $executable -Destination $asset
$hash = (Get-FileHash -LiteralPath $asset -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "Packaged release asset: $asset"
Write-Host "SHA-256: $hash"
