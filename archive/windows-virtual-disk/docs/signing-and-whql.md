# VirtualSSD Windows Driver Signing & WHQL Attestation Pipeline

- **Target**: Windows 11 x64 (Build 22000+) & Windows 10 x64
- **Requirement**: Microsoft Partner Center Hardware Dev Center Attestation Signing

---

## 1. Prerequisites for Kernel Driver Signing

To sign `virtualssd.sys` for production release without requiring users to enable `testsigning`:
1. **EV Code Signing Certificate**: Hardware token (e.g. DigiCert / Sectigo HSM).
2. **Microsoft Partner Center Account**: Registered with EV certificate.
3. **Windows Hardware Lab Kit (HLK)**: Required if submitting for WHQL qualification logo.

---

## 2. Attestation Signing Workflow (Future Work)

> The `.vcxproj` and `package.ddf` referenced below do not exist in this
> repository yet. The miniport prototype is not built or signed, and its I/O path
> is incomplete (see `docs/driver-installation.md`). These commands document the
> intended pipeline, not a currently reproducible build.

```powershell
# 1. Build driver package with WDK MSBuild
msbuild /p:Configuration=Release /p:Platform=x64 drivers/windows-virtual-disk/vssd.vcxproj

# 2. Create CAB package containing .sys and .inf
makecab /f drivers/windows-virtual-disk/package.ddf

# 3. Sign CAB package with EV Certificate
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /sha1 <THUMBPRINT> disk1/virtualssd.cab

# 4. Submit CAB to Microsoft Hardware Dev Center portal for instant attestation signing
# Microsoft signs with Microsoft Windows Hardware Compatibility Publisher signature.

# 5. Download signed package containing Microsoft-signed virtualssd.cat
```

---

## 3. Implemented Path: Windows Virtual Disk API (`virtdisk.dll`)

VirtualSSD currently runs using Microsoft's native signed Virtual Storage Bus
Provider (`storvsp.sys` / `virtdisk.dll`). This requires no custom driver
installation or signing steps. It is the only storage path exercised by the
desktop application in this build.
