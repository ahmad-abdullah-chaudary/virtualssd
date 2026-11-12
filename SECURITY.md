# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 1.0.x   | :white_check_mark: |

## Credential and Secret Handling

VirtualSSD stores provider credentials under `%LOCALAPPDATA%\VirtualSSD\providers\`:

- `providers.json` contains provider ids, kinds, and display names only — no tokens or secrets.
- `cred_<provider_id>.bin` contains the provider secret payload encrypted with Windows DPAPI for the current Windows user (the same scope used by `CryptProtectData` without `CRYPTPROTECT_LOCAL_MACHINE`).
- Writes use a temporary file followed by rename, so a crash cannot leave a half-written credential or index. A corrupt index is preserved as `providers.json.corrupt-<timestamp>` instead of being overwritten.
- Removing a provider deletes its encrypted credential file and provider state (Google Drive resumable-session directory and Telegram SQLite database).
- Disk settings (`settings.json`) contain no secrets and are written with temp-file + rename. Invalid JSON is preserved as `settings.json.corrupt-<timestamp>` and surfaced to the UI.
- Google Drive credentials implement a redacting `Debug` and zeroize secret fields on drop. Telegram configuration does the same and skips secret fields during serialization. Error text returned to the UI redacts the bot token.

### DPAPI scope and limitations

DPAPI encryption is bound to the Windows user profile. A process running as the same logged-in user can request decryption; DPAPI is not a defense against malware already running in that user's session. Credentials are not recoverable after a Windows profile reset or on another machine.

### Data-at-rest protections

Chunk payloads are checksummed with BLAKE3-256. Disks can additionally be created
with client-side AES-256-GCM encryption:

- The random 256-bit key is stored as `disks/<disk_id>/encryption.key`, protected
  with Windows DPAPI for the current user.
- The key is never uploaded. The provider stores ciphertext plus a ciphertext
  checksum.
- The 64-character hexadecimal recovery key is shown only when explicitly
  exported. Anyone with that key and the remote ciphertext can decrypt the disk,
  so store it offline.
- Losing every copy of the key makes the encrypted remote chunks unrecoverable.
  VirtualSSD fails closed and does not regenerate a key.

Disks created without encryption rely on the selected provider (Google Drive or
Telegram) and its account security. Do not treat an unencrypted disk as
end-to-end encrypted storage.

The live local `disk.vhdx` is not encrypted by the chunk layer while it is
attached; client-side encryption protects mirrored chunk payloads only.

### Recovery key handling

Recovery keys are never written to logs or included in manifests. The export
command returns the key to the UI, where the user can copy it. Import accepts 64
hexadecimal characters (an optional `vssd:` prefix and whitespace are
normalized). Importing a key replaces the disk's local key file only after the
disk is confirmed to be marked encrypted.

### Telegram Bot API token

The Telegram Bot API transports the bot token in the URL path. VirtualSSD redacts the token from error messages and never includes it in the secret-free provider index, but the token is inherently visible to the local process and to any HTTP inspection between the process and `api.telegram.org`. Use a dedicated bot token and revoke it if exposure is suspected.

## Reporting a Vulnerability

VirtualSSD handles sensitive block data, authentication tokens, and virtual disk
data. If you discover a security vulnerability, please report it responsibly:

- **Do NOT** open a public GitHub issue.
- Email your findings directly to `info@chaudaryglobal.com` or submit a private security advisory.
- Provide step-by-step reproduction instructions, affected platform/OS build, and attack scenario.
- We acknowledge security reports within 24 hours and aim to release a patch or advisory within 7 days.
