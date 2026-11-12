# Publishes a GitHub release for VirtualSSD.
#
# Tags the current commit, uploads the signed artifacts from ./dist, and
# writes the release notes from docs/release-notes-template.md.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\publish-release.ps1 -Version 1.0.0+1
#
# The GitHub token is read from the Git credential manager, the same place
# `git push` gets it from, so it is never written to disk or to the shell
# history.

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$distDir = Join-Path $repoRoot "dist"
$repo = "ahmad-abdullah-chaudary/virtualssd"
$api = "https://api.github.com"

param()

function Get-GitHubToken {
    $filled = "protocol=https`nhost=github.com`n`n" | git credential fill 2>$null
    $line = $filled | Where-Object { $_ -like "password=*" } | Select-Object -First 1
    if (-not $line) { return $null }
    return ($line -replace '^password=', '').Trim()
}

$token = Get-GitHubToken
if (-not $token) {
    throw "No GitHub credential was found in the git credential manager. Run 'git push' once to store it, or set a token in VIRTUALSSD_GITHUB_TOKEN."
}
if ($env:VIRTUALSSD_GITHUB_TOKEN) { $token = $env:VIRTUALSSD_GITHUB_TOKEN }

$headers = @{
    "Authorization" = "Bearer $token"
    "Accept"        = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"    = "VirtualSSD-Release-Script"
}

# ── Version ───────────────────────────────────────────────────────────────────
$version = $args[0]
if (-not $version) { $version = $env:VIRTUALSSD_RELEASE_VERSION }
if (-not $version) {
    # Fall back to the version recorded by the last build.
    $versionFile = Join-Path $distDir "VERSION.txt"
    if (Test-Path $versionFile) {
        $line = Get-Content $versionFile | Where-Object { $_ -match '^VirtualSSD (.+)$' } | Select-Object -First 1
        if ($line) { $version = ($line -replace '^VirtualSSD ', '').Trim() }
    }
}
if (-not $version) { throw "No release version was given and dist\VERSION.txt does not exist." }
$tag = "v$version"
Write-Host "Publishing $repo release $tag" -ForegroundColor Cyan

# ── Verify the artifacts exist and are signed ─────────────────────────────────
$assets = @(
    @{ Path = (Join-Path $distDir "VirtualSSD-setup.exe"); Name = "VirtualSSD-setup.exe"; Label = "NSIS installer (signed)" },
    @{ Path = (Join-Path $distDir "VirtualSSD.msi");       Name = "VirtualSSD.msi";       Label = "MSI installer (signed)" },
    @{ Path = (Join-Path $distDir "VirtualSSD.exe");       Name = "VirtualSSD.exe";       Label = "Standalone executable (signed)" }
)

$present = @()
foreach ($asset in $assets) {
    if (-not (Test-Path $asset.Path)) {
        Write-Host "  SKIP  $($asset.Name) (not present)" -ForegroundColor DarkYellow
        continue
    }
    $sig = Get-AuthenticodeSignature -FilePath $asset.Path
    $signed = $null -ne $sig.SignerCertificate
    Write-Host "  $(if ($signed) { 'SIGNED' } else { 'UNSIGNED' })  $($asset.Name)  ($([math]::Round((Get-Item $asset.Path).Length/1MB,2)) MB)" -ForegroundColor $(if ($signed) { "Green" } else { "Yellow" })
    $present += $asset
}
if ($present.Count -eq 0) { throw "No release assets were found in $distDir." }

# ── Tag ───────────────────────────────────────────────────────────────────────
Push-Location $repoRoot
$existingTag = git tag --list $tag
if (-not $existingTag) {
    git tag -a $tag -m "VirtualSSD $version"
    Write-Host "Created tag $tag" -ForegroundColor Green
}
$taggedCommit = git rev-list -n 1 $tag
$headCommit = git rev-parse HEAD
if ($taggedCommit -ne $headCommit) {
    Pop-Location
    throw "Tag $tag points at $taggedCommit but HEAD is $headCommit. The working tree must be committed before tagging."
}
git push origin $tag
Pop-Location

# ── Release notes ─────────────────────────────────────────────────────────────
$templatePath = Join-Path $repoRoot "docs\release-notes-template.md"
$changelogPath = Join-Path $repoRoot "CHANGELOG.md"

$notes = if (Test-Path $templatePath) {
    (Get-Content $templatePath -Raw)
} else {
    "# VirtualSSD $version`n`nSee the CHANGELOG for details."
}
$notes = $notes.Replace("{{VERSION}}", $version)

if (Test-Path $changelogPath) {
    $changelog = Get-Content $changelogPath -Raw
    $unreleased = [regex]::Match($changelog, '(?s)## \[Unreleased\]\s*(.*?)(?=\n## \[)').Groups[1].Value
    if ($unreleased.Trim()) {
        $notes = $notes.Replace("{{CHANGELOG}}", $unreleased.Trim())
    } else {
        $notes = $notes.Replace("{{CHANGELOG}}", "See CHANGELOG.md for the full history.")
    }
} else {
    $notes = $notes.Replace("{{CHANGELOG}}", "See CHANGELOG.md for the full history.")
}

# ── Create or update the release ──────────────────────────────────────────────
$bodyFile = Join-Path $env:TEMP ("virtualssd-release-" + [guid]::NewGuid().ToString("N") + ".md")
$notes | Set-Content -Path $bodyFile -Encoding utf8

$existingRelease = Invoke-RestMethod -Uri "$api/repos/$repo/releases/tags/$tag" -Headers $headers -ErrorAction SilentlyContinue
if ($existingRelease) {
    Write-Host "Release $tag already exists; uploading assets to it." -ForegroundColor Yellow
    $releaseId = $existingRelease.id
} else {
    $payload = @{
        tag_name         = $tag
        name             = "VirtualSSD $version"
        body             = $notes
        draft            = $false
        prerelease       = $false
    } | ConvertTo-Json -Depth 5
    $release = Invoke-RestMethod -Uri "$api/repos/$repo/releases" -Method Post -Headers $headers -Body $payload -ContentType "application/json"
    $releaseId = $release.id
    Write-Host "Created release $tag" -ForegroundColor Green
}

# ── Upload assets ─────────────────────────────────────────────────────────────
foreach ($asset in $present) {
    $uploadUrl = "$api/repos/$repo/releases/$releaseId/assets?name=$([uri]::EscapeDataString($asset.Name))"
    $bytes = [System.IO.File]::ReadAllBytes($asset.Path)
    Write-Host "  uploading $($asset.Name) ($([math]::Round($bytes.Length/1MB,2)) MB)..." -ForegroundColor Cyan
    Invoke-RestMethod -Uri $uploadUrl -Method Post -Headers $headers -Body $bytes -ContentType "application/octet-stream" | Out-Null
    Write-Host "  uploaded $($asset.Name)" -ForegroundColor Green
}

Remove-Item $bodyFile -ErrorAction SilentlyContinue

$release = Invoke-RestMethod -Uri "$api/repos/$repo/releases/tags/$tag" -Headers $headers
Write-Host ""
Write-Host "Release published: $($release.html_url)" -ForegroundColor Green
Write-Host "Assets:"
foreach ($a in $release.assets) {
    Write-Host "    $($a.name)  $([math]::Round($a.size/1MB,2)) MB  $($a.browser_download_url)"
}
