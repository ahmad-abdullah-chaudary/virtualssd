# Obtains a Google Drive OAuth 2.0 client id and refresh token for VirtualSSD.
#
# The desktop application asks for a client id and a refresh token rather than
# running the consent flow itself, so that one credential can be moved between
# installations. This script performs the consent flow for you, using PKCE, and
# prints the two values to paste into the Cloud Providers page.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\get-google-drive-token.ps1 `
#       -ClientId "1234567890-abc.apps.googleusercontent.com"
#
# Options:
#   -ClientId      the OAuth client id from the Google Cloud console. When it is
#                   omitted the script asks for it.
#   -ClientSecret  the client secret, when the client has one. Desktop clients
#                   do not need it, and the script works without it.
#   -Scope         defaults to the Drive scope VirtualSSD uses.
#   -Port          the local port for the OAuth redirect. Defaults to 8765.
#
# Nothing is written to disk. The refresh token is printed once, at the end.

param(
    [string]$ClientId,
    [string]$ClientSecret,
    [string]$Scope = "https://www.googleapis.com/auth/drive.file",
    [int]$Port = 8765
)

$ErrorActionPreference = "Stop"

function Write-Head($text) {
    Write-Host ""
    Write-Host "== $text" -ForegroundColor Cyan
}

# Google requires TLS 1.2. Some older Windows builds still negotiate TLS 1.0
# by default, which Google refuses, and the failure looks like a generic
# connection error rather than a protocol problem. Enabling it explicitly
# costs nothing on a modern system.
try {
    [Net.ServicePointManager]::SecurityProtocol = `
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {
    # A locked down system may forbid this. The request below will report a
    # clear error if TLS 1.2 is genuinely unavailable.
}

function Get-RandomBase64Url([int]$bytes) {
    $buffer = New-Object byte[] $bytes
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    $rng.GetBytes($buffer)
    $rng.Dispose()
    [Convert]::ToBase64String($buffer).TrimEnd("=").Replace("+", "-").Replace("/", "_")
}

function Get-Sha256Base64Url([string]$text) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $hash = $sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($text))
    $sha.Dispose()
    [Convert]::ToBase64String($hash).TrimEnd("=").Replace("+", "-").Replace("/", "_")
}

function Get-QueryValue([string]$query, [string]$name) {
    foreach ($pair in $query.TrimStart("?").Split("&")) {
        if ($pair -notmatch "=") { continue }
        $key, $value = $pair.Split("=", 2)
        if ([uri]::UnescapeDataString($key) -eq $name) {
            return [uri]::UnescapeDataString($value)
        }
    }
    return $null
}

# ── 1. The client id ─────────────────────────────────────────────────────────
if (-not $ClientId) {
    Write-Head "Step 1 of 2 - the OAuth client id"
    Write-Host "Create it first:"
    Write-Host "  1. https://console.cloud.google.com/  -> create a project"
    Write-Host "  2. Enable the Google Drive API for the project"
    Write-Host "  3. OAuth consent screen -> External -> add yourself as a test user"
    Write-Host "  4. Credentials -> Create credentials -> OAuth client ID -> Desktop app"
    Write-Host ""
    $ClientId = (Read-Host "Paste the Client ID (xxxx.apps.googleusercontent.com)").Trim()
}

if (-not $ClientId -or $ClientId -notmatch "\.apps\.googleusercontent\.com$") {
    throw "That does not look like an OAuth client id (expected something ending in .apps.googleusercontent.com)."
}

# ── 2. PKCE ─────────────────────────────────────────────────────────────────
$verifier = Get-RandomBase64Url 64
$challenge = Get-Sha256Base64Url $verifier
$redirect = "http://localhost:$Port/"

# ── 3. The consent flow ─────────────────────────────────────────────────────
Write-Head "Step 2 of 2 - the refresh token"
Write-Host "Opening your browser to approve the Drive scope."
Write-Host "Choose the Google account that will hold the virtual disks."
Write-Host "If the browser does not open, copy this URL into it:"
Write-Host ""

$authUrl = "https://accounts.google.com/o/oauth2/v2/auth" +
    "?client_id=" + [uri]::EscapeDataString($ClientId) +
    "&redirect_uri=" + [uri]::EscapeDataString($redirect) +
    "&response_type=code" +
    "&scope=" + [uri]::EscapeDataString($Scope) +
    "&code_challenge=" + [uri]::EscapeDataString($challenge) +
    "&code_challenge_method=S256" +
    "&access_type=offline&prompt=consent&include_granted_scopes=true"

Write-Host $authUrl -ForegroundColor DarkGray
Write-Host ""

$listener = New-Object System.Net.HttpListener
$listener.Prefixes.Add($redirect)
try {
    $listener.Start()
} catch {
    throw "Could not listen on $redirect : $($_.Exception.Message). Try a different -Port."
}

Start-Process $authUrl | Out-Null

$code = $null
$denied = $null
$deadline = (Get-Date).AddMinutes(5)
while (-not $code -and -not $denied -and (Get-Date) -lt $deadline) {
    $context = $listener.GetContext()
    $query = [string]$context.Request.Url.Query
    $code = Get-QueryValue $query "code"
    $denied = Get-QueryValue $query "error"

    if ($code -or $denied) {
        $body = "<html><body style='font-family:Segoe UI,sans-serif;padding:40px'>"
        if ($code) {
            $body += "<h2>VirtualSSD</h2><p>Authorisation received. You can close this tab and return to the terminal.</p>"
        } else {
            $body += "<h2>VirtualSSD</h2><p>Authorisation was not granted ($denied). Return to the terminal.</p>"
        }
        $body += "</body></html>"
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($body)
        $context.Response.ContentLength64 = $bytes.Length
        $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
        $context.Response.Close()
    }
}
$listener.Stop()

if ($denied) {
    throw "The consent was not granted: $denied"
}
if (-not $code) {
    throw "No authorisation code was received within five minutes."
}

# ── 4. Exchange the code for a refresh token ────────────────────────────────
Write-Host ""
Write-Host "Exchanging the authorisation code for a refresh token..."

$tokenParams = @{
    client_id     = $ClientId
    code          = $code
    code_verifier = $verifier
    grant_type    = "authorization_code"
    redirect_uri  = $redirect
}
if ($ClientSecret) {
    $tokenParams["client_secret"] = $ClientSecret
}

try {
    $response = Invoke-RestMethod -Method Post -Uri "https://oauth2.googleapis.com/token" -Body $tokenParams -TimeoutSec 60
} catch {
    # Windows PowerShell 5.1 leaves ErrorDetails.Message empty for a failed
    # Invoke-RestMethod, so the useful text - Google's error_description - has
    # to be read out of the response stream. Without this the script would
    # report only "The remote server returned an error: (401) Unauthorized"
    # and hide the reason, which is usually "redirect_uri_mismatch" or
    # "invalid_grant".
    $detail = ""
    $webResponse = $_.Exception.Response
    if ($webResponse) {
        try {
            $stream = $webResponse.GetResponseStream()
            $reader = New-Object System.IO.StreamReader($stream)
            $body = $reader.ReadToEnd()
            $reader.Dispose()
            if ($body) {
                try {
                    $parsed = $body | ConvertFrom-Json
                    if ($parsed.error_description) {
                        $detail = "$($parsed.error): $($parsed.error_description)"
                    } elseif ($parsed.error) {
                        $detail = $parsed.error
                    } else {
                        $detail = $body
                    }
                } catch {
                    $detail = $body
                }
            }
        } catch {
            $detail = ""
        }
    }
    if (-not $detail) { $detail = $_.Exception.Message }
    throw "The token exchange failed: $detail"
}

if (-not $response.refresh_token) {
    throw "Google did not return a refresh token. Revoke the grant at https://myaccount.google.com/permissions and try again, making sure the consent screen lists you as a test user."
}

# ── 5. Report ───────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "=======================================================" -ForegroundColor Green
Write-Host " Copy these into VirtualSSD -> Cloud Providers" -ForegroundColor Green
Write-Host "=======================================================" -ForegroundColor Green
Write-Host ""
Write-Host "OAuth2 Client ID:"
Write-Host "  $ClientId"
Write-Host ""
Write-Host "OAuth2 Refresh Token:"
Write-Host "  $($response.refresh_token)"
Write-Host ""
Write-Host "Keep the refresh token private. It grants access to the files this"
Write-Host "application creates in your Drive. It is stored on this machine"
Write-Host "encrypted with Windows DPAPI, and is never written to a log."
Write-Host ""
Write-Host "The scope that was requested:"
Write-Host "  $Scope"
Write-Host ""
Write-Host "If the Drive quota shows as not reported, grant the broader scope"
Write-Host "  https://www.googleapis.com/auth/drive"
Write-Host "instead and repeat the flow."
