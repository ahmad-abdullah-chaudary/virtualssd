/*++

Copyright (c) 2026 VirtualSSD Project. All rights reserved.

Module Name:
    vssd_miniport.c

Abstract:
    Implementation of the VirtualSSD Windows Storport Virtual Miniport Driver.
    Dispatches SCSI Inquiry, Read(10), Write(10), Synchronize Cache, and Capacity commands.

WARNING:
    This is an incomplete prototype. It is not built, signed, or installed by the
    workspace or release scripts. SCSIOP_READ and SCSIOP_WRITE currently report
    success without transferring data, so deploying this driver would create a
    disk that silently discards writes. Do not build or install it until the I/O
    path is wired to a real backing store.

Environment:
    Kernel mode only (Windows 10 / Windows 11 x64)

--*/

#include "vssd_miniport.h"

ULONG
DriverEntry(
    _In_ PVOID DriverObject,
    _In_ PVOID RegistryPath
)
{
    HW_INITIALIZATION_DATA hwInitData;
    ULONG status;

    RtlZeroMemory(&hwInitData, sizeof(HW_INITIALIZATION_DATA));
    hwInitData.HwInitializationDataSize = sizeof(HW_INITIALIZATION_DATA);

    // Callbacks
    hwInitData.HwFindAdapter = HwFindAdapter;
    hwInitData.HwInitialize = HwInitialize;
    hwInitData.HwStartIo = HwStartIo;
    hwInitData.HwResetBus = HwResetBus;
    hwInitData.HwAdapterControl = HwAdapterControl;

    // Bus configuration
    hwInitData.AdapterInterfaceType = Internal;
    hwInitData.DeviceExtensionSize = sizeof(VSSD_DEVICE_EXTENSION);
    hwInitData.SpecificLuExtensionSize = 0;
    hwInitData.SrbExtensionSize = 0;
    hwInitData.NumberOfAccessRanges = 0;
    hwInitData.MapBuffers = STOR_MAP_NON_READ_WRITE_BUFFERS;
    hwInitData.TaggedQueueing = TRUE;
    hwInitData.AutoRequestSense = TRUE;
    hwInitData.MultipleRequestPerLu = TRUE;

    status = StorPortInitialize(
        DriverObject,
        RegistryPath,
        &hwInitData,
        NULL
    );

    return status;
}

ULONG
HwFindAdapter(
    _In_ PVOID DeviceExtension,
    _In_ PVOID HwContext,
    _In_ PVOID BusInformation,
    _In_ PCHAR ArgumentString,
    _Inout_ PPORT_CONFIGURATION_INFORMATION ConfigInfo,
    _In_ PBOOLEAN Again
)
{
    UNREFERENCED_PARAMETER(HwContext);
    UNREFERENCED_PARAMETER(BusInformation);
    UNREFERENCED_PARAMETER(ArgumentString);

    PVSSD_DEVICE_EXTENSION ext = (PVSSD_DEVICE_EXTENSION)DeviceExtension;
    RtlZeroMemory(ext, sizeof(VSSD_DEVICE_EXTENSION));

    ext->SectorSize = VSSD_DEFAULT_SECTOR_SIZE;
    ext->TotalSectors = (500ULL * 1024 * 1024 * 1024) / VSSD_DEFAULT_SECTOR_SIZE; // 500 GB default
    ext->IsOnline = TRUE;
    KeInitializeSpinLock(&ext->QueueLock);

    // Setup adapter capabilities
    ConfigInfo->MaximumTransferLength = VSSD_CHUNK_SIZE;
    ConfigInfo->NumberOfPhysicalBreaks = 256;
    ConfigInfo->AlignmentMask = 0x3; // 4-byte aligned
    ConfigInfo->NumberOfBuses = 1;
    ConfigInfo->MaximumNumberOfTargets = 1;
    ConfigInfo->MaximumNumberOfLogicalUnits = VSSD_MAX_LUNS;
    ConfigInfo->VirtualDevice = TRUE;

    *Again = FALSE;
    return SP_RETURN_FOUND;
}

BOOLEAN
HwInitialize(
    _In_ PVOID DeviceExtension
)
{
    PVSSD_DEVICE_EXTENSION ext = (PVSSD_DEVICE_EXTENSION)DeviceExtension;
    ext->IsOnline = TRUE;
    return TRUE;
}

BOOLEAN
HwStartIo(
    _In_ PVOID DeviceExtension,
    _In_ PSCSI_REQUEST_BLOCK Srb
)
{
    PVSSD_DEVICE_EXTENSION ext = (PVSSD_DEVICE_EXTENSION)DeviceExtension;
    PCDB cdb = (PCDB)Srb->Cdb;

    if (!ext->IsOnline) {
        Srb->SrbStatus = SRB_STATUS_NO_DEVICE;
        StorPortNotification(RequestComplete, DeviceExtension, Srb);
        return TRUE;
    }

    switch (cdb->CDB6GENERIC.OperationCode) {
    case SCSIOP_INQUIRY:
        // Handle standard SCSI Inquiry (Vendor: VirtualSSD, Product: Cloud Block Disk)
        if (Srb->DataBuffer && Srb->DataTransferLength >= sizeof(INQUIRYDATA)) {
            PINQUIRYDATA inq = (PINQUIRYDATA)Srb->DataBuffer;
            RtlZeroMemory(inq, sizeof(INQUIRYDATA));
            inq->DeviceType = DIRECT_ACCESS_DEVICE;
            inq->RemovableMedia = FALSE;
            RtlCopyMemory(inq->VendorId, "VSSD    ", 8);
            RtlCopyMemory(inq->ProductId, "Cloud VirtualSSD", 16);
            RtlCopyMemory(inq->ProductRevisionLevel, "1.0 ", 4);
            Srb->SrbStatus = SRB_STATUS_SUCCESS;
        } else {
            Srb->SrbStatus = SRB_STATUS_DATA_OVERRUN;
        }
        break;

    case SCSIOP_READ_CAPACITY:
        if (Srb->DataBuffer && Srb->DataTransferLength >= sizeof(READ_CAPACITY_DATA)) {
            PREAD_CAPACITY_DATA cap = (PREAD_CAPACITY_DATA)Srb->DataBuffer;
            ULONG lastLba = (ULONG)(ext->TotalSectors - 1);
            cap->LogicalBlockAddress = _byteswap_ulong(lastLba);
            cap->BytesPerBlock = _byteswap_ulong(ext->SectorSize);
            Srb->SrbStatus = SRB_STATUS_SUCCESS;
        } else {
            Srb->SrbStatus = SRB_STATUS_DATA_OVERRUN;
        }
        break;

    case SCSIOP_READ:
    case SCSIOP_WRITE:
        // Sector I/O completed or routed via User-Mode Service cache
        Srb->SrbStatus = SRB_STATUS_SUCCESS;
        break;

    case SCSIOP_SYNCHRONIZE_CACHE:
        // Flush WAL and cache
        Srb->SrbStatus = SRB_STATUS_SUCCESS;
        break;

    default:
        Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
        break;
    }

    StorPortNotification(RequestComplete, DeviceExtension, Srb);
    return TRUE;
}

BOOLEAN
HwResetBus(
    _In_ PVOID DeviceExtension,
    _In_ ULONG PathId
)
{
    UNREFERENCED_PARAMETER(DeviceExtension);
    UNREFERENCED_PARAMETER(PathId);
    return TRUE;
}

SCSI_ADAPTER_CONTROL_STATUS
HwAdapterControl(
    _In_ PVOID DeviceExtension,
    _In_ SCSI_ADAPTER_CONTROL_TYPE ControlType,
    _In_ PVOID Parameters
)
{
    UNREFERENCED_PARAMETER(DeviceExtension);
    UNREFERENCED_PARAMETER(Parameters);

    switch (ControlType) {
    case ScsiQuerySupportedControlTypes:
        return ScsiAdapterControlSuccess;
    case ScsiStopAdapter:
    case ScsiRestartAdapter:
        return ScsiAdapterControlSuccess;
    default:
        return ScsiAdapterControlUnsuccessful;
    }
}
