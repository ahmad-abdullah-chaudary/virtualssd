# Creates a self-signed code-signing certificate for VirtualSSD.
#
# A self-signed certificate removes the "unknown publisher" warning for anyone
# who has the certificate installed, and it makes the build reproducible on a
# machine without a commercial code-signing certificate.
#
# It does NOT remove the Microsoft SmartScreen "Windows protected your PC"
# warning for the general public. That warning is reputation-based: a binary
# must be signed by a certificate issued by a trusted CA and downloaded by
# users before Microsoft stops flagging it. To clear SmartScreen for public
# distribution, buy a code-signing certificate (for example from DigiCert,
# Sectigo, or GlobalSign) and install it into CurrentUser\My before running
# the build.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\create-dev-certificate.ps1
#
# The certificate and its private key never leave this machine's certificate
# store and are never written to the repository.

$ErrorActionPreference = "Stop"

$Subject = "CN=Ahmad Abdullah Chaudary, O=Chaudary Global, C=PK"
$Years = 3

Write-Host "Creating a code-signing certificate for VirtualSSD" -ForegroundColor Cyan
Write-Host "Subject: $Subject" -ForegroundColor DarkGray
Write-Host ""

$existing = Get-ChildItem Cert:\CurrentUser\My -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -eq $Subject }

if ($existing) {
    Write-Host "A certificate with this subject already exists:" -ForegroundColor Yellow
    $existing | ForEach-Object { Write-Host "    $($_.Thumbprint)  expires $($_.NotAfter)" -ForegroundColor Yellow }
    Write-Host ""
    Write-Host "Delete it first if you want a fresh one:" -ForegroundColor Yellow
    Write-Host "    Get-ChildItem Cert:\CurrentUser\My | Where-Object { `$_.Subject -eq '$Subject' } | Remove-Item" -ForegroundColor Yellow
    exit 0
}

$cert = New-SelfSignedCertificate `
    -Subject $Subject `
    -Type CodeSigningCert `
    -KeyUsage DigitalSignature `
    -KeyAlgorithm RSA `
    -KeyLength 3072 `
    -HashAlgorithm SHA256 `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -NotAfter (Get-Date).AddYears($Years) `
    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3")

Write-Host "Certificate created." -ForegroundColor Green
Write-Host "    Subject   : $($cert.Subject)"
Write-Host "    Thumbprint: $($cert.Thumbprint)"
Write-Host "    Expires   : $($cert.NotAfter)"
Write-Host ""
Write-Host "To build with it, either:" -ForegroundColor Cyan
Write-Host "    1. Run scripts\build.ps1 - it auto-detects the only code-signing cert, or" -ForegroundColor DarkGray
Write-Host "    2. Set the thumbprint explicitly:" -ForegroundColor DarkGray
Write-Host "       `$env:VIRTUALSSD_CERT_THUMBPRINT = '$($cert.Thumbprint)'" -ForegroundColor DarkGray
Write-Host ""
Write-Host "Trusting the certificate on this machine" -ForegroundColor Cyan
Write-Host "Windows reports a self-signed signature as 'UnknownError' until the" -ForegroundColor DarkGray
Write-Host "certificate is trusted. Add it to the Trusted Root store for the current" -ForegroundColor DarkGray
Write-Host "user (Windows asks for confirmation):" -ForegroundColor DarkGray
Write-Host ""
Write-Host "    certmgr.msc > Trusted Root Certification Authorities >" -ForegroundColor DarkGray
Write-Host "    Import the certificate, or run:" -ForegroundColor DarkGray
Write-Host "      Import-Certificate -FilePath <exported .cer> -CertStoreLocation Cert:\CurrentUser\Root" -ForegroundColor DarkGray
Write-Host ""
Write-Host "This certificate is self-signed. It clears the 'unknown publisher'" -ForegroundColor Yellow
Write-Host "prompt, but SmartScreen will still flag downloads for the public." -ForegroundColor Yellow
