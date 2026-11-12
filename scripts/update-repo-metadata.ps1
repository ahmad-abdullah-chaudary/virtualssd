# Updates the GitHub repository metadata: description, topics, and the social
# preview image. This is what a visitor sees before they scroll, so it is
# worth getting right.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\update-repo-metadata.ps1

$ErrorActionPreference = "Stop"

$repo = "ahmad-abdullah-chaudary/virtualssd"
$api = "https://api.github.com"

function Get-GitHubToken {
    $filled = "protocol=https`nhost=github.com`n`n" | git credential fill 2>$null
    $line = $filled | Where-Object { $_ -like "password=*" } | Select-Object -First 1
    if (-not $line) { return $null }
    return ($line -replace '^password=', '').Trim()
}

$token = Get-GitHubToken
if ($env:VIRTUALSSD_GITHUB_TOKEN) { $token = $env:VIRTUALSSD_GITHUB_TOKEN }
if (-not $token) {
    throw "No GitHub credential was found in the git credential manager. Run 'git push' once to store it, or set VIRTUALSSD_GITHUB_TOKEN."
}

$headers = @{
    "Authorization" = "Bearer $token"
    "Accept"        = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent"    = "VirtualSSD-Metadata-Script"
}

# ── Description and topics ───────────────────────────────────────────────────
$description = "Your cloud account, mounted as a real Windows disk. VirtualSSD creates a genuine VHDX block device backed by your own Google Drive or Telegram storage, with AES-256-GCM encryption, BLAKE3-verified snapshots, and concurrent transfers."

$topics = @(
    "block-storage",
    "vhd",
    "vhdx",
    "windows",
    "tauri",
    "rust",
    "cloud-storage",
    "google-drive",
    "telegram",
    "encryption",
    "aes-gcm",
    "blake3",
    "storage-engine",
    "self-hosted",
    "windows-11",
    "privacy"
)

$payload = @{
    description = $description
    homepage    = "https://github.com/$repo"
    has_wiki    = $true
    has_issues  = $true
    has_projects = $false
} | ConvertTo-Json -Depth 5

$file = Join-Path $env:TEMP ("virtualssd-repo-meta-" + [guid]::NewGuid().ToString("N") + ".json")
[System.IO.File]::WriteAllText($file, $payload, (New-Object System.Text.UTF8Encoding($false)))
try {
    Invoke-RestMethod -Uri "$api/repos/$repo" -Method Patch -Headers $headers -InFile $file -ContentType "application/json; charset=utf-8" | Out-Null
} finally {
    Remove-Item $file -ErrorAction SilentlyContinue
}
Write-Host "Updated the repository description." -ForegroundColor Green

# Topics have their own endpoint; the repository endpoint does not apply them.
# The body shape is { "names": [ ... ] }.
$topicsFile = Join-Path $env:TEMP ("virtualssd-topics-" + [guid]::NewGuid().ToString("N") + ".json")
$topicsBody = @{ names = $topics } | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText($topicsFile, $topicsBody, (New-Object System.Text.UTF8Encoding($false)))
try {
    Invoke-RestMethod -Uri "$api/repos/$repo/topics" -Method Put -Headers $headers -InFile $topicsFile -ContentType "application/json; charset=utf-8" | Out-Null
    Write-Host "Updated the repository topics." -ForegroundColor Green
} finally {
    Remove-Item $topicsFile -ErrorAction SilentlyContinue
}

# ── Social preview image ─────────────────────────────────────────────────────
# GitHub picks the social preview up from `.github/social-preview.png` in the
# default branch automatically, so the image only has to be committed. It is
# committed with the rest of the repository; nothing is uploaded here.
$previewPath = Join-Path (Split-Path -Parent $PSScriptRoot) ".github\social-preview.png"
if (Test-Path $previewPath) {
    Write-Host "The social preview image is committed at .github\social-preview.png." -ForegroundColor Green
} else {
    Write-Host "No social preview image was found at .github\social-preview.png." -ForegroundColor Yellow
}

# ── Report ───────────────────────────────────────────────────────────────────
$updated = Invoke-RestMethod -Uri "$api/repos/$repo" -Headers $headers
Write-Host ""
Write-Host "description: $($updated.description)"
Write-Host "topics:      $($updated.topics -join ', ')"
Write-Host "url:         $($updated.html_url)"
