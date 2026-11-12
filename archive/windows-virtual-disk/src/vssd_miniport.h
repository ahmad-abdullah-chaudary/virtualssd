/*++

Copyright (c) 2026 VirtualSSD Project. All rights reserved.

Module Name:
    vssd_miniport.h

Abstract:
    Header definitions for the VirtualSSD Windows Storport Virtual Miniport Driver.
    Exposes authentic physical block storage devices to the Windows kernel storage stack.

Environment:
    Kernel mode only (Storport.sys miniport)

--*/

#pragma once

#include <ntddk.h>
#include <storport.h>
#include <ntddstor.h>
#include <ntdddisk.h>

#define VSSD_DRIVER_VERSION_MAJOR 1
#define VSSD_DRIVER_VERSION_MINOR 0
#define VSSD_MAX_LUNS             16
#define VSSD_DEFAULT_SECTOR_SIZE  512
#define VSSD_CHUNK_SIZE           (4 * 1024 * 1024)

// Custom IOCTLs for User-Mode Storage Service communication
#define FSCTL_VSSD_BASE           0x8800
#define IOCTL_VSSD_MAP_CHUNK      CTL_CODE(FSCTL_VSSD_BASE, 0x801, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)
#define IOCTL_VSSD_UNMAP_CHUNK    CTL_CODE(FSCTL_VSSD_BASE, 0x802, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)
#define IOCTL_VSSD_GET_TELEMETRY  CTL_CODE(FSCTL_VSSD_BASE, 0x803, METHOD_BUFFERED, FILE_ANY_ACCESS)

typedef struct _VSSD_DEVICE_EXTENSION {
    ULONG DeviceNumber;
    ULONG SectorSize;
    ULONG64 TotalSectors;
    BOOLEAN IsOnline;
    BOOLEAN IsReadOnly;
    PVOID ChunkMapTable;
    KSPIN_LOCK QueueLock;
} VSSD_DEVICE_EXTENSION, *PVSSD_DEVICE_EXTENSION;

// Miniport Callback Prototypes
DRIVER_INITIALIZE DriverEntry;

ULONG
HwFindAdapter(
    _In_ PVOID DeviceExtension,
    _In_ PVOID HwContext,
    _In_ PVOID BusInformation,
    _In_ PCHAR ArgumentString,
    _Inout_ PPORT_CONFIGURATION_INFORMATION ConfigInfo,
    _In_ PBOOLEAN Again
);

BOOLEAN
HwInitialize(
    _In_ PVOID DeviceExtension
);

BOOLEAN
HwStartIo(
    _In_ PVOID DeviceExtension,
    _In_ PSCSI_REQUEST_BLOCK Srb
);

BOOLEAN
HwResetBus(
    _In_ PVOID DeviceExtension,
    _In_ ULONG PathId
);

SCSI_ADAPTER_CONTROL_STATUS
HwAdapterControl(
    _In_ PVOID DeviceExtension,
    _In_ SCSI_ADAPTER_CONTROL_TYPE ControlType,
    _In_ PVOID Parameters
);
