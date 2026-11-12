# Connecting Google Drive

- **Revision**: 1.0 (2026-11-12)

VirtualSSD talks to Google Drive through the official Drive REST API v3. It
needs two values: an **OAuth 2.0 client id** and an **OAuth 2.0 refresh
token**.

The desktop application deliberately does not run the consent flow itself. It
asks for a refresh token so that one credential can be moved to another
machine, and so that the consent screen can be reviewed in the Google account
where you want the storage to live.

---

## What you need

| | |
|---|---|
| A Google account | The one whose Drive will hold the virtual disks |
| A Google Cloud project | Free tier is fine |
| The Drive API enabled | In that project |
| A **Desktop app** OAuth client | Gives a client id, and no secret is required |
| The `drive.file` scope | Access to the files VirtualSSD creates |

`drive.file` is the narrow scope. VirtualSSD only ever touches the chunk and
manifest files it creates itself, so it cannot read the rest of your Drive.

---

## Step 1 - Create the client id

1. Open <https://console.cloud.google.com/> and **create a project**.
2. With the project selected, open **APIs & Services → Library** and enable
   **Google Drive API**.
3. Open **APIs & Services → OAuth consent screen**.
   - User type: **External**.
   - Fill in the app name and your email address.
   - Under **Test users**, add **your own Google account**. This matters: a
     consent screen left in *Testing* mode issues refresh tokens that expire
     after seven days, and only listed test users can authorise it.
   - Save.
4. Open **APIs & Services → Credentials → Create credentials → OAuth client
   ID**.
   - Application type: **Desktop app**.
   - Name it anything, for example `VirtualSSD`.
   - Copy the **Client ID**. It looks like
     `1234567890-abcdef.apps.googleusercontent.com`.
   - A **client secret** is also shown. A desktop client does not need it, and
     VirtualSSD works without it. Keep it if you prefer to paste it in a
     future version of the app.

---

## Step 2 - Get the refresh token

The helper script performs the consent flow with PKCE and prints the two
values:

```powershell
cd "<the VirtualSSD folder>"
powershell -ExecutionPolicy Bypass -File .\scripts\get-google-drive-token.ps1 `
    -ClientId "1234567890-abcdef.apps.googleusercontent.com"
```

It opens your browser, asks you to approve the Drive scope, and then prints:

```
OAuth2 Client ID:
  1234567890-abcdef.apps.googleusercontent.com

OAuth2 Refresh Token:
  1//0eXXXXXX...
```

Nothing is written to disk and no secret is stored by the script. The refresh
token is printed once.

Options:

| Option | Meaning |
|---|---|
| `-ClientId` | The client id from step 1. Asked for if omitted. |
| `-ClientSecret` | Only if you want to send one. Not needed for a desktop client. |
| `-Scope` | Defaults to `https://www.googleapis.com/auth/drive.file`. |
| `-Port` | The local port for the OAuth redirect. Defaults to `8765`. |

### If the browser does not open

The script prints the URL. Copy it into a browser and approve it there. The
redirect goes to `http://localhost:8765/`, which the script is listening on.

### If you would rather use the OAuth Playground

1. Open <https://developers.google.com/oauthplayground/>.
2. Click the **gear** (Settings) and tick **Use your own OAuth credentials**.
3. Enter your **client id** and, if you have one, the **client secret**.
4. Enter the scope `https://www.googleapis.com/auth/drive.file`.
5. Click **Authorize APIs**, choose your account, and approve.
6. Type your **client id** into the *OAuth Client ID* field, then click
   **Exchange authorization code for tokens**.
7. Copy the **Refresh token**.

---

## Step 3 - Add the provider in VirtualSSD

1. Open **Cloud Providers** in VirtualSSD.
2. Under **Connect Google Drive**, fill in:
   - **Display name** — anything, for example `Personal Google Drive`.
   - **OAuth2 Client ID** — the client id from step 1.
   - **OAuth2 Refresh Token** — the refresh token from step 2.
3. Click **Connect**.

The application verifies the credentials against the Drive API before storing
them. If verification succeeds, the credentials are encrypted with **Windows
DPAPI** for the current user and written to
`%LOCALAPPDATA%\VirtualSSD\providers\cred_<id>.bin`. The provider index
(`providers.json`) contains no secrets, and the token never appears in a log.

If verification fails, the credentials are **not** stored.

---

## Troubleshooting

### "Failed to refresh Google Drive token"

Google rejected the refresh token. The usual reasons:

- The consent screen is still in **Testing** mode, so the token already
  expired. Publish the consent screen to **Production**, or add your account
  as a test user and repeat the flow.
- The grant was revoked at
  <https://myaccount.google.com/permissions>. Repeat the flow.
- The refresh token was copied with a trailing space or line break.

### "Google Drive connected but offline"

The provider was stored, but a later verification failed. Click **Verify** on
the provider row and read the message.

### The Drive quota shows "not reported by Google"

`drive.file` does not grant access to the account storage quota, so the Drive
API does not return it. The application reports that honestly rather than
inventing a number. To see the real quota, repeat the flow with the broader
scope `https://www.googleapis.com/auth/drive`.

### "Cannot reach Google Drive API"

The request never reached Google. See
[troubleshooting.md](troubleshooting.md#7-network-connection-failure-when-adding-a-provider).
A proxy is supported through the standard `HTTPS_PROXY` environment variable.

### The disk appears in the app but is not on Drive yet

A disk is created locally first; its manifest is published immediately, but the
chunks are uploaded when you mirror it. Press **Mirror Snapshot** after
unmounting, or **Mirror While Mounted** to mirror a volume that is in use.

---

## What ends up where

| | |
|---|---|
| Chunk data | Your Drive, as files named `chunk_<index>_<blake3>.bin` |
| Manifest | Your Drive, as `manifest_<disk-id>.json` |
| Provider credentials | `%LOCALAPPDATA%\VirtualSSD\providers\cred_<id>.bin`, DPAPI-encrypted |
| Provider index | `%LOCALAPPDATA%\VirtualSSD\providers\providers.json`, no secrets |
| Upload sessions | `%LOCALAPPDATA%\VirtualSSD\providers\gdrive_<id>\`, so an interrupted upload resumes |

Nothing is stored on VirtualSSD's servers, because VirtualSSD has no servers.
