Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$documents = @(
    "README.md",
    "README.zh-CN.md",
    "CHANGELOG.md",
    "CHANGELOG.zh-CN.md",
    "SECURITY.md",
    "SECURITY.zh-CN.md",
    "PRIVACY.md",
    "PRIVACY.zh-CN.md",
    "CONTRIBUTING.md",
    "CONTRIBUTING.zh-CN.md",
    "GLOSSARY.md",
    "THIRD_PARTY_NOTICES.md",
    "docs\ARCHITECTURE.md",
    "docs\USER_GUIDE.md",
    "docs\USER_GUIDE.zh-CN.md"
) + (Get-ChildItem -LiteralPath (Join-Path $repoRoot "docs\releases") -Filter "*.md" | ForEach-Object FullName)

foreach ($document in $documents) {
    $documentPath = if ([System.IO.Path]::IsPathRooted($document)) {
        $document
    } else {
        Join-Path $repoRoot $document
    }
    $content = Get-Content -Raw -LiteralPath $documentPath
    foreach ($match in [regex]::Matches($content, '!?\[[^\]]+\]\(([^)]+)\)')) {
        $target = $match.Groups[1].Value.Trim().Trim('<', '>')
        if ($target -match '^(?:https?://|mailto:|#)') {
            continue
        }
        $relative = $target.Split('#')[0]
        if ($relative -eq "") {
            continue
        }
        $relative = [System.Uri]::UnescapeDataString($relative)
        $resolved = [System.IO.Path]::GetFullPath((Join-Path (Split-Path $documentPath) $relative))
        if (-not (Test-Path -LiteralPath $resolved)) {
            throw "Broken relative link in ${documentPath}: $target"
        }
    }
}

Write-Host "Release documentation links verified."
