# VirtualSSD Release Build Script
#
# Builds the Rust workspace, runs the non-elevated test suites, produces the
# signed Tauri desktop application and its installers, and assembles release
# artifacts under ./dist.
#
# Versioning
#   Each build gets a unique build number (1.0.0+N) taken from the counter in
#   apps/desktop/src-tauri/BUILD_NUMBER. The counter is incremented on every
#   run, so the executable, the Windows file version resource, the installer
#   filename, and the release tag all change with every build.
#
# Signing
#   The build is signed with a code-signing certificate so Windows does not
#   flag the binaries as coming from an unknown publisher. The certificate is
#   resolved in this order:
#     1. VIRTUALSSD_CERT_THUMBPRINT  - thumbprint of a cert in CurrentUser\My
#     2. VIRTUALSSD_CERT_SUBJECT     - subject of a cert in CurrentUser\My
#     3. The single Code Signing cert in CurrentUser\My, if there is exactly one
#   If no certificate is found the build still completes, but the signing step
#   reports that the artifacts are unsigned instead of silently shipping them.

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriDir = Join-Path $repoRoot "apps\desktop\src-tauri"
$distDir = Join-Path $repoRoot "dist"

function Write-Step($text) {
    Write-Host ""
    Write-Host "==> $text" -ForegroundColor Cyan
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " VirtualSSD Release Build                                 " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# ── 1. Version ────────────────────────────────────────────────────────────────
Write-Step "Resolving the build version"
$versionOutput = & (Join-Path $PSScriptRoot "version.ps1")
# version.ps1 writes a human-readable line and then the bare version. Take the
# last line that actually looks like a version, so the informational output
# cannot be mistaken for the value.
$version = ($versionOutput | Where-Object { $_ -match '^\d+\.\d+\.\d+\+\d+$' } | Select-Object -Last 1)
if (-not $version) { throw "version.ps1 did not produce a version. Output was: $($versionOutput -join ' | ')" }
$version = $version.Trim()

# Tauri's config is merged with this JSON overlay, so the source file keeps a
# stable "1.0.0" and the build number lives only in the produced artifact.
$env:TAURI_CONFIG = "{`"version`":`"$version`"}"

# Cargo package versions cannot carry semver build metadata in every context,
# so the Rust crates keep 1.0.0 while the shipped application version (which
# is what Windows and the installers display) is the per-build value.
$env:VIRTUALSSD_BUILD_VERSION = $version

# ── 2. Verify the workspace ───────────────────────────────────────────────────
Write-Step "Checking the Rust workspace"
Push-Location $repoRoot
cargo check --workspace --all-targets
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo check failed." }

Write-Step "Running the Rust test suites"
cargo test --workspace
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo test failed." }
Pop-Location

# ── 3. Frontend type check ────────────────────────────────────────────────────
Write-Step "Type checking the frontend"
Push-Location (Join-Path $repoRoot "apps\desktop")
npx tsc --noEmit
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "frontend type check failed." }
Pop-Location

# ── 4. Build the Tauri application and installers ─────────────────────────────
Write-Step "Building the Tauri desktop application"
Push-Location (Join-Path $repoRoot "apps\desktop")
npx tauri build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "tauri build failed." }
Pop-Location

# ── 5. Locate the signing certificate ─────────────────────────────────────────
Write-Step "Resolving the code-signing certificate"
$thumbprint = $env:VIRTUALSSD_CERT_THUMBPRINT
$subject = $env:VIRTUALSSD_CERT_SUBJECT

function Find-CodeSigningCert {
    param($Thumbprint, $Subject)

    $certs = @(Get-ChildItem Cert:\CurrentUser\My -ErrorAction SilentlyContinue |
        Where-Object { $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date) })

    $codeSigning = @($certs | Where-Object {
        $eku = $_.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId }
        $eku -contains '1.3.6.1.5.5.7.3.3'
    })
    if ($codeSigning.Count -eq 0) { $codeSigning = $certs }

    if ($Thumbprint) {
        $match = $codeSigning | Where-Object { $_.Thumbprint -eq $Thumbprint.Trim() } | Select-Object -First 1
        if (-not $match) { throw "No usable certificate with thumbprint $Thumbprint was found in CurrentUser\My." }
        return $match
    }
    if ($Subject) {
        $match = $codeSigning | Where-Object { $_.Subject -like "*$Subject*" } | Select-Object -First 1
        if (-not $match) { throw "No usable certificate matching '$Subject' was found in CurrentUser\My." }
        return $match
    }
    if ($codeSigning.Count -eq 1) { return $codeSigning[0] }
    if ($codeSigning.Count -gt 1) {
        Write-Host "Multiple certificates are available; set VIRTUALSSD_CERT_THUMBPRINT to choose one:" -ForegroundColor Yellow
        $codeSigning | ForEach-Object { Write-Host "    $($_.Thumbprint)  $($_.Subject)" -ForegroundColor Yellow }
        throw "Refusing to guess which certificate to use."
    }
    return $null
}

$cert = Find-CodeSigningCert -Thumbprint $thumbprint -Subject $subject

# ── 6. Sign the binaries ──────────────────────────────────────────────────────
Write-Step "Signing the release binaries"
$targetDir = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
$exe = Join-Path $targetDir "release\virtualssd-desktop.exe"
$msi = Join-Path $targetDir "release\bundle\msi\VirtualSSD_1.0.0_x64_en-US.msi"
$nsis = Join-Path $targetDir "release\bundle\nsis\VirtualSSD_1.0.0_x64-setup.exe"

# The NSIS and MSI bundle names embed the version, so locate them by pattern.
$msi = (Get-ChildItem (Split-Path $msi) -Filter "*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1).FullName
$nsis = (Get-ChildItem (Split-Path $nsis) -Filter "*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1).FullName

$timestampUrl = if ($env:VIRTUALSSD_TIMESTAMP_URL) { $env:VIRTUALSSD_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }

function Sign-File {
    param($Path, $Certificate, $Timestamp)

    if (-not (Test-Path $Path)) {
        Write-Host "    SKIP  $Path (not produced by this build)" -ForegroundColor DarkYellow
        return $false
    }

    try {
        # Set-AuthenticodeSignature is the supported Windows signing API for
        # this toolchain; it produces a real Authenticode signature and an RFC
        # 3161 timestamp, exactly like signtool.exe.
        $result = Set-AuthenticodeSignature -FilePath $Path `
            -Certificate $Certificate `
            -TimestampServer $Timestamp `
            -HashAlgorithm SHA256 `
            -ErrorAction Stop

        # Read the signature back. A self-signed certificate produces
        # `UnknownError` because its root is not in the trust store, but the
        # signature itself is still present and valid — that is why the
        # presence of the signer and the timestamp is checked as well.
        $sig = Get-AuthenticodeSignature -FilePath $Path
        $hasSigner = $null -ne $sig.SignerCertificate
        $hasStamp = $null -ne $sig.TimeStamperCertificate

        if (-not $hasSigner) {
            Write-Host "    FAIL  $Path -> no signer certificate was embedded ($($result.StatusMessage))" -ForegroundColor Red
            return $false
        }
        if (-not $hasStamp) {
            Write-Host "    FAIL  $Path -> the signature carries no RFC 3161 timestamp" -ForegroundColor Red
            return $false
        }

        if ($sig.Status -eq 'Valid') {
            Write-Host "    OK    $([System.IO.Path]::GetFileName($Path)) signed and trusted ($($sig.SignerCertificate.Subject))" -ForegroundColor Green
        } else {
            Write-Host "    OK    $([System.IO.Path]::GetFileName($Path)) signed by $($sig.SignerCertificate.Subject)" -ForegroundColor Green
            Write-Host "          status '$($sig.Status)': $($sig.StatusMessage)" -ForegroundColor DarkYellow
            Write-Host "          the signature is embedded and timestamped; the chain is untrusted because the" -ForegroundColor DarkYellow
            Write-Host "          certificate is self-signed. A CA-issued certificate reports 'Valid'." -ForegroundColor DarkYellow
        }
        return $true
    } catch {
        Write-Host "    FAIL  $Path -> $($_.Exception.Message)" -ForegroundColor Red
        return $false
    }
}

$signedAll = $true
if ($cert) {
    Write-Host "    Certificate: $($cert.Subject)"
    Write-Host "    Thumbprint : $($cert.Thumbprint)"
    Write-Host "    Expires    : $($cert.NotAfter)"
    Write-Host "    Timestamper: $timestampUrl"
    Write-Host ""

    foreach ($target in @($exe, $nsis, $msi)) {
        if (-not (Sign-File -Path $target -Certificate $cert -Timestamp $timestampUrl)) { $signedAll = $false }
    }
} else {
    Write-Host "    No code-signing certificate was found in CurrentUser\My." -ForegroundColor Yellow
    Write-Host "    The build will be UNSIGNED and Windows SmartScreen will flag it." -ForegroundColor Yellow
    Write-Host "    Create one with scripts\create-dev-certificate.ps1, or set" -ForegroundColor Yellow
    Write-Host "    VIRTUALSSD_CERT_THUMBPRINT to a certificate you already have." -ForegroundColor Yellow
    $signedAll = $false
}

# Copies an artifact into ./dist.
#
# The destination may be running: a copy fails outright when the file is
# locked. Writing to a temporary name and then replacing the target works
# because Windows opens a running executable with a delete share.
function Install-Artifact {
    param([string]$Source, [string]$Destination)

    if (-not (Test-Path $Source)) { return $false }

    $staging = "$Destination.new"
    Remove-Item $staging -Force -ErrorAction SilentlyContinue
    Copy-Item $Source $staging -Force

    try {
        Remove-Item $Destination -Force -ErrorAction SilentlyContinue
        Move-Item $staging $Destination -Force
    } catch {
        # The destination is still held by a running process. Fall back to a
        # plain copy and report it, so the build does not silently produce a
        # stale artifact.
        Remove-Item $staging -Force -ErrorAction SilentlyContinue
        Copy-Item $Source $Destination -Force
        Write-Host "    (the previous $Destination was in use and could not be replaced cleanly)" -ForegroundColor Yellow
    }
    return $true
}

# ── 7. Assemble the release artifacts ─────────────────────────────────────────
Write-Step "Assembling release artifacts in ./dist"
New-Item -ItemType Directory -Force -Path $distDir | Out-Null

# The application links the WebView2 loader dynamically, so Windows looks for
# `WebView2Loader.dll` next to the executable. The installers ship it, but a
# bare copy of the executable does not, and Windows then reports
# "The code execution cannot proceed because WebView2Loader.dll was not
# found". The DLL is copied next to the portable executable and into the
# portable archive for exactly that reason.
$webviewLoader = Join-Path $targetDir "release\WebView2Loader.dll"

if (Test-Path $exe) {
    Install-Artifact -Source $exe -Destination (Join-Path $distDir "VirtualSSD.exe") | Out-Null
    Write-Host "    dist\VirtualSSD.exe"

    if (Test-Path $webviewLoader) {
        Install-Artifact -Source $webviewLoader -Destination (Join-Path $distDir "WebView2Loader.dll") | Out-Null
        Write-Host "    dist\WebView2Loader.dll"
    } else {
        Write-Host "    WebView2Loader.dll was not found at $webviewLoader" -ForegroundColor Red
        throw "The WebView2 loader is missing. The portable executable would not start without it."
    }
}
if ($nsis) {
    Install-Artifact -Source $nsis -Destination (Join-Path $distDir "VirtualSSD-setup.exe") | Out-Null
    Write-Host "    dist\VirtualSSD-setup.exe"
}
if ($msi) {
    Install-Artifact -Source $msi -Destination (Join-Path $distDir "VirtualSSD.msi") | Out-Null
    Write-Host "    dist\VirtualSSD.msi"
}

# A portable archive that contains everything the application needs, so it can
# be unpacked and run without installing anything.
$portableStage = Join-Path $env:TEMP ("virtualssd-portable-" + [guid]::NewGuid().ToString("N"))
try {
    New-Item -ItemType Directory -Force -Path $portableStage | Out-Null
    Copy-Item (Join-Path $distDir "VirtualSSD.exe") $portableStage -Force
    Copy-Item (Join-Path $distDir "WebView2Loader.dll") $portableStage -Force
    $portableZip = Join-Path $distDir "VirtualSSD-portable.zip"
    Remove-Item $portableZip -ErrorAction SilentlyContinue
    Compress-Archive -Path (Join-Path $portableStage "*") -DestinationPath $portableZip -CompressionLevel Optimal
    Write-Host "    dist\VirtualSSD-portable.zip"
} finally {
    Remove-Item $portableStage -Recurse -Force -ErrorAction SilentlyContinue
}

# Verify that the portable copy can actually resolve its loader. Without this
# the executable fails at startup with a confusing "not found" error.
if (Test-Path (Join-Path $distDir "VirtualSSD.exe")) {
    $portableExe = Join-Path $distDir "VirtualSSD.exe"
    $bytes = [System.IO.File]::ReadAllBytes($portableExe)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    if ($text -match 'WebView2Loader\.dll' -and -not (Test-Path (Join-Path $distDir "WebView2Loader.dll"))) {
        throw "The portable executable needs WebView2Loader.dll next to it, but it is missing."
    }
    Write-Host "    Verified: the portable executable resolves WebView2Loader.dll." -ForegroundColor Green
}

$versionFile = Join-Path $distDir "VERSION.txt"
@(
    "VirtualSSD $version",
    "Build number: $version",
    "Company: Chaudary Global",
    "Developer: Ahmad Abdullah Chaudary",
    "Country: PK",
    "Signed: $signedAll",
    "Built: $(Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK')"
) | Set-Content -Path $versionFile -Encoding utf8
Write-Host "    dist\VERSION.txt"

Write-Host ""
if ($signedAll) {
    Write-Host "Build complete and SIGNED. Artifacts in ./dist" -ForegroundColor Green
} else {
    Write-Host "Build complete but UNSIGNED. Artifacts in ./dist" -ForegroundColor Yellow
    Write-Host "Windows will flag these binaries as from an unknown publisher." -ForegroundColor Yellow
}
Write-Host "Note: no Windows service or kernel driver is shipped; VHDX attachment uses the Windows Virtual Disk API." -ForegroundColor Yellow
