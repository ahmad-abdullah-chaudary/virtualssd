# Quarantined Prototypes

The directories in this folder are **not part of the workspace, build, installer,
or release artifacts**. They are retained only as historical reference.

## `storage-service/`

An early service scaffold. It enumerates disk directories and waits for Ctrl+C.
It never called `StartServiceCtrlDispatcher`, never opened a named pipe or HTTP
listener, and was never used by the desktop application. Registering it with the
Service Control Manager fails with error 1053. Do not build or deploy it.

## `windows-virtual-disk/`

An early Storport miniport prototype. Its `SCSIOP_READ`/`SCSIOP_WRITE` path
completed requests without transferring data, so installing it would create a
disk that silently discards writes. It has no working project file, is not
signed, and cannot be deployed without a WDK, an EV certificate, and WHQL
attestation. Do not build, sign, or install it.

The supported storage path is the Windows Virtual Disk API (`virtdisk.dll`) used
by `apps/desktop/src-tauri`, documented in `docs/driver-installation.md`.
