//! Volume Shadow Copy snapshots of a live VHDX.
//!
//! Mirroring an unmounted disk is simple: the VHDX file is not open, so
//! reading it is trivially consistent. Mirroring a *mounted* disk is not, and
//! that is what makes VirtualSSD usable as a persistent drive rather than a
//! backup tool.
//!
//! A Volume Shadow Copy solves it. The VSS service asks the file system
//! writer to flush its cache and then takes a point-in-time image of the
//! volume that holds the VHDX. The shadow copy contains the VHDX exactly as
//! it was at that moment, while the live volume keeps running.
//!
//! The snapshot is created through the `Win32_ShadowCopy` WMI class, which is
//! the supported way to ask the VSS service for a client-accessible copy
//! without linking against the VSS backup COM interfaces.
//!
//! # Consistency
//!
//! The VHDX file is written by the Microsoft Virtual Disk driver (`vhdmp`),
//! which pushes its data blocks through the file system cache. The NTFS VSS
//! writer flushes that cache before the image is taken, so the shadow copy
//! contains the data that was written. The driver keeps its block allocation
//! table in memory and writes it out lazily, so the copy can be *slightly
//! behind* the live volume — but it is never a mixture of two moments,
//! because the driver writes a data block before it marks the block
//! allocated. A shadow copy is therefore either consistent or a little
//! stale, and in both cases it is a valid VHDX.
//!
//! To narrow the window further, the caller flushes the guest file system
//! into the VHD with `FSCTL_FLUSH_VOLUME` before the snapshot is taken.
//!
//! Nothing here can damage the live volume: the snapshot is a copy, and the
//! original file is only ever read.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VssError {
    #[error("Volume shadow copies require administrator privileges")]
    NotElevated,

    #[error("Could not determine which volume holds {0}")]
    VolumeNotFound(String),

    #[error("The volume shadow copy could not be created: {0}")]
    SnapshotFailed(String),

    #[error("The volume shadow copy could not be released: {0}")]
    ReleaseFailed(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// A snapshot of the volume that holds a file.
///
/// The snapshot is released when this value is dropped.
#[derive(Debug)]
pub struct VolumeSnapshot {
    /// The shadow copy device path, for example
    /// `\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy3`.
    device_path: String,
    /// The shadow copy identifier, used to release it.
    snapshot_id: String,
    /// The volume the snapshot was taken of, for example `C:\`.
    volume: String,
}

impl VolumeSnapshot {
    /// Takes a snapshot of the volume that contains `file`.
    ///
    /// This requires administrator privileges.
    pub fn capture_for_file(file: &Path) -> Result<Self, VssError> {
        let volume = volume_root(file)?;
        Self::capture_volume(&volume)
    }

    /// Takes a snapshot of a volume given as `C:\`.
    pub fn capture_volume(volume: &str) -> Result<Self, VssError> {
        let normalized = normalize_volume(volume);
        let (device_path, snapshot_id) = create_shadow_copy(&normalized)?;

        tracing::info!("Created volume shadow copy {snapshot_id} of {normalized} ({device_path})");

        Ok(Self {
            device_path,
            snapshot_id,
            volume: normalized,
        })
    }

    /// The shadow copy device path.
    pub fn device_path(&self) -> &str {
        &self.device_path
    }

    /// The shadow copy identifier.
    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    /// The volume the snapshot was taken of.
    pub fn volume(&self) -> &str {
        &self.volume
    }

    /// Maps a path inside the live volume to the same path inside the shadow
    /// copy.
    ///
    /// The result can be opened with ordinary file APIs and is read-only.
    pub fn shadowed_path(&self, original: &Path) -> Result<PathBuf, VssError> {
        let original = original.to_path_buf();
        let relative = original.strip_prefix(&self.volume).map_err(|_| {
            VssError::VolumeNotFound(format!(
                "{} is not inside the snapshotted volume {}",
                original.display(),
                self.volume
            ))
        })?;

        // The shadow copy device path has no trailing separator, so the
        // relative path is appended directly.
        let mut shadowed = PathBuf::from(&self.device_path);
        shadowed.push(relative);
        Ok(shadowed)
    }

    /// Releases the shadow copy.
    ///
    /// A shadow copy holds a differential area on the volume. Leaving it
    /// behind would consume disk space, so it is always released — including
    /// when the caller drops the value.
    pub fn release(&self) {
        if let Err(e) = release_shadow_copy(&self.snapshot_id) {
            tracing::warn!(
                "The volume shadow copy {} could not be released: {e}",
                self.snapshot_id
            );
        }
    }
}

impl Drop for VolumeSnapshot {
    fn drop(&mut self) {
        self.release();
    }
}

/// Returns the volume root that holds `path`, for example `C:\`.
pub fn volume_root(path: &Path) -> Result<String, VssError> {
    use windows::Win32::Storage::FileSystem::GetVolumePathNameW;

    let wide = to_wide(path.as_os_str());
    // MAX_PATH is the documented limit for this call.
    let mut buffer = vec![0u16; 261];
    unsafe { GetVolumePathNameW(windows::core::PCWSTR(wide.as_ptr()), &mut buffer) }.map_err(
        |e| {
            VssError::VolumeNotFound(format!(
                "{} is not on a mounted volume: {e}",
                path.display()
            ))
        },
    )?;

    // The call null-terminates the result.
    let length = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    buffer.truncate(length);
    Ok(String::from_utf16_lossy(&buffer))
}

/// Normalises a volume string to the `X:\` form the VSS service expects.
fn normalize_volume(volume: &str) -> String {
    let trimmed = volume.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return volume.to_string();
    }
    format!("{}\\", trimmed)
}

fn to_wide(text: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// Releases COM on the current thread when it goes out of scope.
struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

/// Creates a client-accessible shadow copy of a volume through WMI.
///
/// Returns the shadow copy device path and its identifier.
fn create_shadow_copy(volume: &str) -> Result<(String, String), VssError> {
    use windows::core::{Interface, BSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IDispatch, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Variant::{VT_BOOL, VT_BSTR};
    use windows::Win32::System::Wmi::{
        ISWbemLocator, ISWbemObject, ISWbemServices, IWbemClassObject, SWbemLocator,
    };

    // COM has to be initialised on the thread that makes the calls.
    let mut com_guard = None;
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_err() && hr != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
            return Err(VssError::SnapshotFailed(format!(
                "COM could not be initialised: {hr:?}"
            )));
        }
        // Only balance the COM reference count when this call initialised COM.
        // RPC_E_CHANGED_MODE means the thread was already initialised by
        // someone else, and uninitialising it here would break them.
        if hr.is_ok() {
            com_guard = Some(ComGuard);
        }
    }
    let _com = com_guard;

    unsafe {
        let locator: ISWbemLocator = CoCreateInstance(&SWbemLocator, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| VssError::SnapshotFailed(format!("WMI could not be started: {e}")))?;

        let empty = BSTR::from("");
        let services: ISWbemServices = locator
            .ConnectServer(
                &BSTR::from("."),
                &BSTR::from("root\\cimv2"),
                None::<&BSTR>,
                None::<&BSTR>,
                None::<&BSTR>,
                None::<&BSTR>,
                0,
                None::<&IDispatch>,
            )
            .map_err(|e| VssError::SnapshotFailed(format!("WMI could not be reached: {e}")))?;

        // The class object for Win32_ShadowCopy, and a fresh instance of it to
        // carry the method's input parameters.
        let class_object: ISWbemObject = services
            .Get(&BSTR::from("Win32_ShadowCopy"), 0, None::<&IDispatch>)
            .map_err(|e| {
                VssError::SnapshotFailed(format!(
                    "The Win32_ShadowCopy class is not available: {e}"
                ))
            })?;

        let class_object: IWbemClassObject = class_object.cast().map_err(|e| {
            VssError::SnapshotFailed(format!(
                "The Win32_ShadowCopy class could not be prepared: {e}"
            ))
        })?;

        let instance: IWbemClassObject = class_object.SpawnInstance(0).map_err(|e| {
            VssError::SnapshotFailed(format!("A shadow copy request could not be created: {e}"))
        })?;

        let volume_variant = windows::core::VARIANT::from(volume);
        instance
            .Put(&BSTR::from("Volume"), 0, &volume_variant, VT_BSTR.0 as i32)
            .map_err(|e| {
                VssError::SnapshotFailed(format!("The 'Volume' parameter was rejected: {e}"))
            })?;

        let context_variant = windows::core::VARIANT::from("ClientAccessible");
        instance
            .Put(
                &BSTR::from("Context"),
                0,
                &context_variant,
                VT_BSTR.0 as i32,
            )
            .map_err(|e| {
                VssError::SnapshotFailed(format!("The 'Context' parameter was rejected: {e}"))
            })?;

        let differential_variant = windows::core::VARIANT::from(false);
        instance
            .Put(
                &BSTR::from("Differential"),
                0,
                &differential_variant,
                VT_BOOL.0 as i32,
            )
            .map_err(|e| {
                VssError::SnapshotFailed(format!("The 'Differential' parameter was rejected: {e}"))
            })?;

        // The method takes its parameters as a dispatch object.
        let parameters: IDispatch = instance.cast().map_err(|e| {
            VssError::SnapshotFailed(format!(
                "The shadow copy request could not be prepared: {e}"
            ))
        })?;

        let result: ISWbemObject = services
            .ExecMethod(
                &BSTR::from("Win32_ShadowCopy"),
                &BSTR::from("Create"),
                &parameters,
                0,
                None::<&IDispatch>,
            )
            .map_err(|e| {
                VssError::SnapshotFailed(format!(
                    "The volume shadow copy could not be created: {e}"
                ))
            })?;

        let result: IWbemClassObject = result.cast().map_err(|e| {
            VssError::SnapshotFailed(format!("The shadow copy result could not be read: {e}"))
        })?;

        // A non-zero ReturnValue is the WMI method's own error code.
        let return_value = read_wmi_i32(&result, "ReturnValue")?;
        if return_value != 0 {
            return Err(VssError::SnapshotFailed(format!(
                "The volume shadow copy request failed with WMI error {return_value}"
            )));
        }

        let device_path = read_wmi_string(&result, "DeviceObject")?;
        let snapshot_id = read_wmi_string(&result, "ID")?;

        if device_path.is_empty() {
            return Err(VssError::SnapshotFailed(
                "The volume shadow copy was created but exposed no device path".to_string(),
            ));
        }

        let _ = empty;
        Ok((device_path, snapshot_id))
    }
}

/// Releases a shadow copy by its identifier.
fn release_shadow_copy(snapshot_id: &str) -> Result<(), VssError> {
    // `vssadmin` is the supported administrative tool for removing a shadow
    // copy, and it is present on every Windows installation.
    let output = std::process::Command::new("vssadmin.exe")
        .args([
            "delete",
            "shadows",
            &format!("/shadow={snapshot_id}"),
            "/quiet",
        ])
        .output()
        .map_err(|e| VssError::ReleaseFailed(format!("vssadmin could not be run: {e}")))?;

    if !output.status.success() {
        return Err(VssError::ReleaseFailed(format!(
            "vssadmin exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim()
        )));
    }
    Ok(())
}

/// Reads a string property from a WMI object.
unsafe fn read_wmi_string(
    object: &windows::Win32::System::Wmi::IWbemClassObject,
    name: &str,
) -> Result<String, VssError> {
    let mut variant = windows::core::VARIANT::default();
    object
        .Get(
            &windows::core::BSTR::from(name),
            0,
            &mut variant,
            None,
            None,
        )
        .map_err(|e| {
            VssError::SnapshotFailed(format!("The '{name}' result could not be read: {e}"))
        })?;

    let value = windows::core::BSTR::try_from(&variant).map_err(|e| {
        VssError::SnapshotFailed(format!("The '{name}' result is not a string: {e}"))
    })?;
    Ok(value.to_string())
}

/// Reads a 32-bit integer property from a WMI object.
unsafe fn read_wmi_i32(
    object: &windows::Win32::System::Wmi::IWbemClassObject,
    name: &str,
) -> Result<i32, VssError> {
    let mut variant = windows::core::VARIANT::default();
    object
        .Get(
            &windows::core::BSTR::from(name),
            0,
            &mut variant,
            None,
            None,
        )
        .map_err(|e| {
            VssError::SnapshotFailed(format!("The '{name}' result could not be read: {e}"))
        })?;
    Ok(i32::try_from(&variant).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(volume: &str) -> VolumeSnapshot {
        VolumeSnapshot {
            device_path: "\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy3".to_string(),
            snapshot_id: "00000000-0000-0000-0000-000000000000".to_string(),
            volume: volume.to_string(),
        }
    }

    #[test]
    fn volumes_are_normalised_to_a_drive_letter_root() {
        assert_eq!(normalize_volume("C:\\"), "C:\\");
        assert_eq!(normalize_volume("C:"), "C:\\");
        assert_eq!(normalize_volume("C:\\"), "C:\\");
    }

    #[test]
    fn a_shadowed_path_keeps_the_relative_location() {
        let shadowed = snapshot("C:\\")
            .shadowed_path(Path::new("C:\\Users\\me\\disk.vhdx"))
            .expect("the path is inside the volume");
        assert_eq!(
            shadowed.to_string_lossy(),
            "\\\\?\\GLOBALROOT\\Device\\HarddiskVolumeShadowCopy3\\Users\\me\\disk.vhdx"
        );
    }

    #[test]
    fn a_path_outside_the_volume_is_rejected() {
        let result = snapshot("C:\\").shadowed_path(Path::new("D:\\other\\disk.vhdx"));
        assert!(result.is_err(), "a path on another volume must be rejected");
    }
}
