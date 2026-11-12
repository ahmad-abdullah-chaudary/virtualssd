# Computes the per-build version for VirtualSSD.
#
# The marketing version is 1.0.0. Every build gets a unique build number so
# the artifact, the Windows file version resource, and the installer filename
# all change with every build, as required for release traceability.
#
# The build number is the number of commits on the current branch, so the
# version of a build maps exactly onto the commit it was built from: the same
# source always produces the same version, and every committed change produces
# a new one. When the build runs outside a git checkout (a source tarball, or
# a machine without git), a local counter file is used instead so the build
# still succeeds and reports an honest version.

$ErrorActionPreference = "Stop"

$tauriDir = Join-Path $PSScriptRoot "..\apps\desktop\src-tauri"
$counterFile = Join-Path $tauriDir "BUILD_NUMBER"
$baseVersion = "1.0.0"

function Get-CommitCount {
    try {
        $inside = & git rev-parse --is-inside-work-tree 2>$null
        if ($LASTEXITCODE -ne 0 -or $inside.Trim() -ne "true") { return $null }
        $count = & git rev-list --count HEAD 2>$null
        if ($LASTEXITCODE -ne 0 -or -not $count) { return $null }
        $parsed = 0
        if (-not [int]::TryParse($count.Trim(), [ref]$parsed)) { return $null }
        if ($parsed -lt 1) { return $null }
        return $parsed
    } catch {
        return $null
    }
}

$commitCount = Get-CommitCount
if ($commitCount) {
    $number = $commitCount
    $source = "git commit count"
} else {
    if (-not (Test-Path $counterFile)) {
        Set-Content -Path $counterFile -Value "1" -Encoding ascii
    }
    $current = [int]((Get-Content $counterFile -Raw).Trim())
    $number = $current + 1
    Set-Content -Path $counterFile -Value $number -Encoding ascii
    $source = "local counter"
}

# 1.0.0+N -> semver build metadata. tauri-build maps this onto the fourth
# component of the Windows FILEVERSION/PRODUCTVERSION resource.
$version = "$baseVersion+$number"

Write-Host "VirtualSSD version: $version (build $number, from $source)"
Write-Output $version
