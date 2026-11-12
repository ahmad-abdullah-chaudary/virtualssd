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

param(
    [string]$Version,
    # The commit the artifacts were built from. It defaults to HEAD, but a
    # release must point at the commit the binary was actually compiled from,
    # which is not always the newest commit when tooling changes are added
    # after a build.
    [string]$Commit
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$distDir = Join-Path $repoRoot "dist"
$repo = "ahmad-abdullah-chaudary/virtualssd"
$api = "https://api.github.com"

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
if (-not $Version) { $Version = $env:VIRTUALSSD_RELEASE_VERSION }
if (-not $Version) {
    # Fall back to the version recorded by the last build.
    $versionFile = Join-Path $distDir "VERSION.txt"
    if (Test-Path $versionFile) {
        $line = Get-Content $versionFile | Where-Object { $_ -match '^VirtualSSD (.+)$' } | Select-Object -First 1
        if ($line) { $Version = ($line -replace '^VirtualSSD ', '').Trim() }
    }
}
if (-not $Version) { throw "No release version was given and dist\VERSION.txt does not exist." }
$tag = "v$Version"
Write-Host "Publishing $repo release $tag" -ForegroundColor Cyan

# ── Verify the artifacts exist and are signed ─────────────────────────────────
$assets = @(
    @{ Path = (Join-Path $distDir "VirtualSSD-setup.exe"); Name = "VirtualSSD-setup.exe"; Label = "NSIS installer (signed)" },
    @{ Path = (Join-Path $distDir "VirtualSSD.msi");       Name = "VirtualSSD.msi";       Label = "MSI installer (signed)" },
    @{ Path = (Join-Path $distDir "VirtualSSD.exe");       Name = "VirtualSSD.exe";       Label = "Standalone executable (signed) - also needs WebView2Loader.dll" },
    @{ Path = (Join-Path $distDir "VirtualSSD-portable.zip"); Name = "VirtualSSD-portable.zip"; Label = "Portable bundle - executable and WebView2Loader.dll (signed)" }
)

$present = @()
foreach ($asset in $assets) {
    if (-not (Test-Path $asset.Path)) {
        Write-Host "  SKIP  $($asset.Name) (not present)" -ForegroundColor DarkYellow
        continue
    }

    # Only Windows executable formats can carry an Authenticode signature.
    # The portable archive is a zip, so the executable inside it is the signed
    # artifact and the archive itself is not signed.
    $signable = @('.exe', '.msi', '.dll', '.cab') -contains ([System.IO.Path]::GetExtension($asset.Path).ToLower())
    if (-not $signable) {
        Write-Host "  N/A   $($asset.Name)  ($([math]::Round((Get-Item $asset.Path).Length/1MB,2)) MB) - archive, not signable; the executable inside is signed" -ForegroundColor DarkGray
        $present += $asset
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
if (-not $Commit) { $Commit = (git rev-parse HEAD).Trim() }
$existingTag = git tag --list $tag
if (-not $existingTag) {
    git tag -a $tag $Commit -m "VirtualSSD $Version"
    Write-Host "Created tag $tag at $Commit" -ForegroundColor Green
}
$taggedCommit = git rev-list -n 1 $tag
if ($taggedCommit -ne $Commit) {
    Pop-Location
    throw "Tag $tag points at $taggedCommit but the artifacts were built from $Commit."
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
$notes = $notes.Replace("{{VERSION}}", $Version)

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
# Write UTF-8 without a byte-order mark: GitHub's JSON parser rejects the BOM.
[System.IO.File]::WriteAllText($bodyFile, $notes, (New-Object System.Text.UTF8Encoding($false)))

$existingRelease = $null
try {
    $existingRelease = Invoke-RestMethod -Uri "$api/repos/$repo/releases/tags/$tag" -Headers $headers -ErrorAction Stop
} catch {
    # A release for this tag does not exist yet, which is the normal case.
    $existingRelease = $null
}
if ($existingRelease) {
    Write-Host "Release $tag already exists; uploading assets to it." -ForegroundColor Yellow
    $releaseId = $existingRelease.id
    $release = $existingRelease
} else {
    $payload = @{
        tag_name         = $tag
        name             = "VirtualSSD $Version"
        body             = $notes
        draft            = $false
        prerelease       = $false
    } | ConvertTo-Json -Depth 5

    # Send the JSON from a file encoded as UTF-8 without a byte-order mark.
    # Windows PowerShell mangles a string body that contains non-ASCII
    # characters, which makes GitHub reject the payload as malformed JSON.
    $payloadFile = Join-Path $env:TEMP ("virtualssd-release-payload-" + [guid]::NewGuid().ToString("N") + ".json")
    [System.IO.File]::WriteAllText($payloadFile, $payload, (New-Object System.Text.UTF8Encoding($false)))
    try {
        $release = Invoke-RestMethod -Uri "$api/repos/$repo/releases" -Method Post -Headers $headers -InFile $payloadFile -ContentType "application/json; charset=utf-8"
    } finally {
        Remove-Item $payloadFile -ErrorAction SilentlyContinue
    }
    $releaseId = $release.id
    Write-Host "Created release $tag" -ForegroundColor Green
}

# ── Upload assets ─────────────────────────────────────────────────────────────
# The upload URL comes from the release itself. GitHub returns a template
# like ".../assets{?name,label}" that must be expanded per asset.
$uploadTemplate = $release.upload_url
if (-not $uploadTemplate) {
    throw "The release did not return an upload_url."
}

foreach ($asset in $present) {
    $label = [uri]::EscapeDataString($asset.Label)
    $name = [uri]::EscapeDataString($asset.Name)
    $uploadUrl = $uploadTemplate.Replace('{?name,label}', "?name=$name&label=$label")
    $uploadUrl = $uploadUrl.Replace('{name,label}', "?name=$name&label=$label")

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
