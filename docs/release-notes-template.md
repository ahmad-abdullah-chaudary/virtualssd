# Release Notes — VirtualSSD {{VERSION}}

**Company:** Chaudary Global · **Developer:** Ahmad Abdullah Chaudary · **Country:** PK

---

## Downloads

| Asset | Description |
|---|---|
| `VirtualSSD-setup.exe` | NSIS installer. Runs the setup wizard, registers the Start Menu shortcut and the uninstaller. |
| `VirtualSSD.msi` | MSI installer for managed and enterprise deployment. |
| `VirtualSSD.exe` | The standalone, portable executable. No installation required. |

All three artifacts carry the same Authenticode signature and the same
version resource (`{{VERSION}}`).

## Verification

Before installing, confirm the signature:

```powershell
Get-AuthenticodeSignature .\VirtualSSD-setup.exe | Format-List Status, SignerCertificate
```

`Status` should read `Valid`. The `SignerCertificate` subject is
`CN=Ahmad Abdullah Chaudary, O=Chaudary Global, C=PK`.

> **SmartScreen note.** If Windows SmartScreen shows "Windows protected your PC",
> click **More info** → **Run anyway**. That warning is reputation-based: it
> appears for any newly published binary until Microsoft has seen enough
> downloads. It is not a sign of tampering — verify the signature instead.

## Versioning

The marketing version is `1.0.0`. The build number appended to it (`{{VERSION}}`)
changes with every build and identifies the exact commit the binary was built
from. The same number appears in the Windows file properties, the installer
filename, and the application's sidebar footer.

## What is in this release

{{CHANGELOG}}

## System requirements

- Windows 11 x64 or Windows 10 x64 (build 19044 or later)
- The Microsoft Edge WebView2 Runtime (preinstalled on current Windows)
- Administrator privileges for mounting and dismounting VHDX volumes

## First run

1. Launch VirtualSSD. It starts unelevated so provider setup and diagnostics
   work without elevation.
2. Open **Cloud Providers** and add your Google Drive OAuth client and refresh
   token, or your Telegram bot token and storage chat id.
3. Open **Disks & Volumes** and create a disk.
4. Use **Restart as administrator** (header or Settings page) before mounting
   a volume — attaching a VHDX requires elevation.

## Documentation

- [README](https://github.com/ahmad-abdullah-chaudary/virtualssd#readme)
- [Architecture decision record](https://github.com/ahmad-abdullah-chaudary/virtualssd/blob/main/docs/architecture.md)
- [Release process](https://github.com/ahmad-abdullah-chaudary/virtualssd/blob/main/docs/release-process.md)
- [Troubleshooting](https://github.com/ahmad-abdullah-chaudary/virtualssd/blob/main/docs/troubleshooting.md)
