param(
    [string]$CacheDirectory = (Join-Path $env:TEMP "browser-harbor\cache"),
    [int]$ValidDays = 7
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cacheFile = Join-Path $CacheDirectory "releases.json"
$output = Join-Path $repoRoot "assets\trusted-metadata.json"

function Get-GitHubJson {
    param([string]$Uri)

    $candidates = @(
        $Uri,
        "https://gh-proxy.com/$Uri",
        "https://ghfast.top/$Uri",
        "https://ghproxy.net/$Uri"
    )
    foreach ($candidate in $candidates) {
        try {
            return Invoke-RestMethod -Uri $candidate -Headers @{
                "User-Agent" = "BrowserHarbor-Metadata-Builder"
            } -TimeoutSec 20
        } catch {
            continue
        }
    }
    throw "could not read GitHub metadata: $Uri"
}

function New-Artifact {
    param(
        [string]$FileName,
        [string]$Format,
        [long]$Size,
        [string]$Digest,
        [string[]]$Urls,
        [string]$SignatureUrl = $null
    )

    $algorithm, $value = $Digest.Split(":", 2)
    [ordered]@{
        fileName = $FileName
        format = $Format
        size = $Size
        integrity = [ordered]@{
            algorithm = $algorithm.ToLowerInvariant()
            value = $value.ToLowerInvariant()
        }
        urls = $Urls
        signatureUrl = $SignatureUrl
    }
}

function New-Descriptor {
    param(
        [string]$Target,
        [string]$Version,
        [string]$Channel,
        [string]$CpuBuild,
        [string]$Provider,
        [object]$Artifact,
        [string]$Placement,
        [string]$Source,
        [string]$PublishedAt = $null,
        [string]$InnerArchive = $null
    )

    [ordered]@{
        target = $Target
        version = $Version
        channel = $Channel
        cpuBuild = $CpuBuild
        provider = $Provider
        artifact = $Artifact
        placement = $Placement
        innerArchive = $InnerArchive
        source = $Source
        publishedAt = $PublishedAt
    }
}

$cache = Get-Content -Raw -LiteralPath $cacheFile | ConvertFrom-Json
$releases = [System.Collections.Generic.Dictionary[string, object]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($property in $cache.PSObject.Properties) {
    $descriptor = $property.Value.descriptor
    $artifact = $descriptor.artifact
    if ($null -eq $artifact.integrity) {
        $artifactPath = Join-Path $CacheDirectory $artifact.fileName
        if (-not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
            throw "artifact has no digest and is missing locally: $($artifact.fileName)"
        }
        $hash = (Get-FileHash -LiteralPath $artifactPath -Algorithm SHA256).Hash
        $artifact.integrity = [ordered]@{
            algorithm = "sha256"
            value = $hash.ToLowerInvariant()
        }
    }
    $key = "$($descriptor.target)|$($descriptor.channel)|$($descriptor.cpuBuild)"
    $releases[$key] = $descriptor
}

# Edge MSI entries are obsolete: the Enterprise MSI is only a bootstrapper.
# Keep the signed manifest free of a fallback that the portable installer cannot consume.
@($releases.Keys | Where-Object { $_ -like "edge|*" }) | ForEach-Object {
    $releases.Remove($_)
}

Write-Host "Edge Stable is resolved at install time through Microsoft's CDP API because its signed URL is short-lived."

$iceReleases = Get-GitHubJson "https://api.github.com/repos/adonais/iceweasel/releases?per_page=30"
$iceRelease = $iceReleases | Where-Object {
    -not $_.draft -and -not $_.prerelease -and $_.tag_name -match "esr"
} | Sort-Object {
    [version](($_.tag_name -replace "^v", "") -replace "[^0-9.].*$", "")
} -Descending | Select-Object -First 1
$iceAsset = $iceRelease.assets | Where-Object {
    $_.name -match "win64" -and $_.name -match "\.7z$"
} | Select-Object -First 1
$iceKey = "iceweasel|esr|x64"
if (-not $releases.ContainsKey($iceKey)) {
    $releases[$iceKey] = New-Descriptor `
        -Target "iceweasel" `
        -Version ($iceRelease.tag_name -replace "^v", "") `
        -Channel "esr" `
        -CpuBuild "x64" `
        -Provider "git_hub_release" `
        -Artifact (New-Artifact `
            -FileName $iceAsset.name `
            -Format "sevenzip" `
            -Size $iceAsset.size `
            -Digest $iceAsset.digest `
            -Urls @($iceAsset.browser_download_url)) `
        -Placement "native_package" `
        -Source $iceRelease.html_url `
        -PublishedAt $iceRelease.published_at
}

$thoriumRelease = Get-GitHubJson "https://api.github.com/repos/gz83/thorium/releases/latest"
$thoriumVersion = $thoriumRelease.tag_name.TrimStart("M")
foreach ($cpu in @("sse3", "sse4", "avx", "avx2", "avx512")) {
    $key = "thorium|stable|$cpu"
    if ($releases.ContainsKey($key)) {
        continue
    }
    $asset = $thoriumRelease.assets | Where-Object {
        $_.name -match "^Thorium_$($cpu.ToUpperInvariant())_$([regex]::Escape($thoriumVersion))\.zip$"
    } | Select-Object -First 1
    if ($null -eq $asset) {
        throw "Thorium asset was not found for $cpu"
    }
    $releases[$key] = New-Descriptor `
        -Target "thorium" `
        -Version $thoriumVersion `
        -Channel "stable" `
        -CpuBuild $cpu `
        -Provider "git_hub_release" `
        -Artifact (New-Artifact `
            -FileName $asset.name `
            -Format "zip" `
            -Size $asset.size `
            -Digest $asset.digest `
            -Urls @($asset.browser_download_url)) `
        -Placement "native_package" `
        -Source $thoriumRelease.html_url `
        -PublishedAt $thoriumRelease.published_at
}

foreach ($descriptor in $releases.Values) {
    if ($null -eq $descriptor.artifact.integrity) {
        throw "release has no integrity value: $($descriptor.target)"
    }
}

$chromePlusRelease = Get-GitHubJson "https://api.github.com/repos/Bush2021/chrome_plus/releases/latest"
$chromePlusAsset = $chromePlusRelease.assets | Where-Object {
    $_.name.StartsWith("Chrome++_v") -and $_.name.EndsWith(".7z")
} | Select-Object -First 1
$setdllAsset = $chromePlusRelease.assets | Where-Object name -eq "setdll.7z" | Select-Object -First 1
$libportableRelease = Get-GitHubJson "https://api.github.com/repos/adonais/libportable/releases/latest"
$libportableAsset = $libportableRelease.assets | Where-Object name -eq "portable_bin.7z" | Select-Object -First 1

$now = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$metadata = [ordered]@{
    schemaVersion = 1
    generatedAt = $now
    expiresAt = $now + ($ValidDays * 86400)
    releases = @($releases.Values | Sort-Object target, channel, cpuBuild)
    chromePlus = [ordered]@{
        version = $chromePlusRelease.tag_name
        artifact = New-Artifact `
            -FileName $chromePlusAsset.name `
            -Format "sevenzip" `
            -Size $chromePlusAsset.size `
            -Digest $chromePlusAsset.digest `
            -Urls @($chromePlusAsset.browser_download_url)
        setdllArtifact = New-Artifact `
            -FileName $setdllAsset.name `
            -Format "sevenzip" `
            -Size $setdllAsset.size `
            -Digest $setdllAsset.digest `
            -Urls @($setdllAsset.browser_download_url)
    }
    libportable = [ordered]@{
        version = $libportableRelease.tag_name
        artifact = New-Artifact `
            -FileName $libportableAsset.name `
            -Format "sevenzip" `
            -Size $libportableAsset.size `
            -Digest $libportableAsset.digest `
            -Urls @($libportableAsset.browser_download_url)
    }
}

$json = $metadata | ConvertTo-Json -Depth 12
[System.IO.File]::WriteAllText(
    $output,
    $json + [Environment]::NewLine,
    [System.Text.UTF8Encoding]::new($false)
)
Write-Host "Wrote signed metadata source to $output"
