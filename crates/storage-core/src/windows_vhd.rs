//! Windows Virtual Hard Disk lifecycle using `virtdisk.dll`.
//!
//! Creates a real VHDX file, attaches it to the Windows storage stack, and
//! formats/assigns it a drive letter. This is the supported block-device path
//! for the desktop application. Every operation fails closed: if a VHDX cannot
//! be attached or formatted, the attachment is removed and an error is returned.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tracing::{error, info};
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_NOT_FOUND, ERROR_SUCCESS, HANDLE,
    WIN32_ERROR,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, PSECURITY_DESCRIPTOR, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::GetLogicalDrives;
use windows::Win32::Storage::Vhd::{
    AttachVirtualDisk, CreateVirtualDisk, DetachVirtualDisk, GetVirtualDiskPhysicalPath,
    OpenVirtualDisk, ATTACH_VIRTUAL_DISK_FLAG_NO_DRIVE_LETTER,
    ATTACH_VIRTUAL_DISK_FLAG_PERMANENT_LIFETIME, CREATE_VIRTUAL_DISK_FLAG_NONE,
    CREATE_VIRTUAL_DISK_PARAMETERS, CREATE_VIRTUAL_DISK_PARAMETERS_0,
    CREATE_VIRTUAL_DISK_PARAMETERS_0_1, CREATE_VIRTUAL_DISK_VERSION_2,
    DETACH_VIRTUAL_DISK_FLAG_NONE, OPEN_VIRTUAL_DISK_FLAG_NONE, VIRTUAL_DISK_ACCESS_ATTACH_RW,
    VIRTUAL_DISK_ACCESS_DETACH, VIRTUAL_DISK_ACCESS_GET_INFO, VIRTUAL_DISK_ACCESS_NONE,
    VIRTUAL_STORAGE_TYPE, VIRTUAL_STORAGE_TYPE_DEVICE_VHDX, VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const VHDX_FILE_NAME: &str = "disk.vhdx";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const POWER_SHELL_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone)]
pub struct AttachedDiskInfo {
    pub disk_number: u32,
    pub device_path: String,
    pub drive_letter: char,
    pub vhdx_path: PathBuf,
}

/// Returns true when the current process has an elevated (administrator) token.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );

        let _ = CloseHandle(token);
        result.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// Creates a dynamic VHDX with the requested virtual size. The file must not
/// already exist; callers should check before calling this function.
pub fn create_vhdx(image_path: &Path, size_bytes: u64) -> Result<(), String> {
    if size_bytes < 3 * 1024 * 1024 {
        return Err("VHDX size must be at least 3 MiB.".to_string());
    }
    if size_bytes > 64 * 1024 * 1024 * 1024 * 1024u64 {
        return Err("VHDX size exceeds the 64 TiB maximum.".to_string());
    }
    if image_path.exists() {
        return Err(format!("VHDX already exists: {}", image_path.display()));
    }
    if let Some(parent) = image_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create VHDX directory: {e}"))?;
    }

    let wide_path = to_wide(image_path);
    let storage_type = VIRTUAL_STORAGE_TYPE {
        DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
        VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
    };
    let parameters = CREATE_VIRTUAL_DISK_PARAMETERS {
        Version: CREATE_VIRTUAL_DISK_VERSION_2,
        Anonymous: CREATE_VIRTUAL_DISK_PARAMETERS_0 {
            Version2: CREATE_VIRTUAL_DISK_PARAMETERS_0_1 {
                UniqueId: GUID::zeroed(),
                MaximumSize: size_bytes,
                BlockSizeInBytes: 0,
                SectorSizeInBytes: 0,
                PhysicalSectorSizeInBytes: 0,
                ParentPath: PCWSTR::null(),
                SourcePath: PCWSTR::null(),
                OpenFlags: OPEN_VIRTUAL_DISK_FLAG_NONE,
                ParentVirtualStorageType: VIRTUAL_STORAGE_TYPE::default(),
                SourceVirtualStorageType: VIRTUAL_STORAGE_TYPE::default(),
                ResiliencyGuid: GUID::zeroed(),
            },
        },
    };

    info!(
        "Creating dynamic VHDX at {} ({} bytes)",
        image_path.display(),
        size_bytes
    );

    let mut handle = HANDLE::default();
    let result = unsafe {
        CreateVirtualDisk(
            &storage_type,
            PCWSTR(wide_path.as_ptr()),
            VIRTUAL_DISK_ACCESS_NONE,
            PSECURITY_DESCRIPTOR::default(),
            CREATE_VIRTUAL_DISK_FLAG_NONE,
            0,
            &parameters,
            None,
            &mut handle,
        )
    };

    if result != ERROR_SUCCESS {
        let message = describe_error("CreateVirtualDisk", result);
        let _ = std::fs::remove_file(image_path);
        return Err(message);
    }

    let _ = unsafe { CloseHandle(handle) };

    if !image_path.is_file() {
        return Err(format!(
            "CreateVirtualDisk reported success but no VHDX exists at {}",
            image_path.display()
        ));
    }

    Ok(())
}

/// Attaches a VHDX and returns its real `\\.\PhysicalDriveN` identity.
pub fn attach_vhdx(image_path: &Path) -> Result<AttachedDiskInfo, String> {
    if !image_path.is_file() {
        return Err(format!("VHDX not found: {}", image_path.display()));
    }
    if !is_elevated() {
        return Err(
            "Attaching a virtual disk requires administrator privileges. Restart VirtualSSD as administrator."
                .to_string(),
        );
    }

    let abs_path = std::fs::canonicalize(image_path)
        .map_err(|e| format!("Failed to resolve VHDX path: {e}"))?;
    let wide_path = to_wide(&abs_path);
    let storage_type = vhdx_storage_type();

    let mut handle = HANDLE::default();
    let open_access = VIRTUAL_DISK_ACCESS_ATTACH_RW | VIRTUAL_DISK_ACCESS_GET_INFO;
    let result = unsafe {
        OpenVirtualDisk(
            &storage_type,
            PCWSTR(wide_path.as_ptr()),
            open_access,
            OPEN_VIRTUAL_DISK_FLAG_NONE,
            None,
            &mut handle,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(describe_error("OpenVirtualDisk", result));
    }

    let attach_flags =
        ATTACH_VIRTUAL_DISK_FLAG_NO_DRIVE_LETTER | ATTACH_VIRTUAL_DISK_FLAG_PERMANENT_LIFETIME;
    let result = unsafe {
        AttachVirtualDisk(
            handle,
            PSECURITY_DESCRIPTOR::default(),
            attach_flags,
            0,
            None,
            None,
        )
    };
    if result != ERROR_SUCCESS {
        let _ = unsafe { CloseHandle(handle) };
        return Err(describe_error("AttachVirtualDisk", result));
    }

    let mut device_path = None;
    for _ in 0..20 {
        match unsafe { physical_path_for_handle(handle) } {
            Ok(path) => {
                device_path = Some(path);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }

    let _ = unsafe { CloseHandle(handle) };

    let device_path = match device_path {
        Some(path) => path,
        None => {
            let _ = detach_vhdx(&abs_path);
            return Err(
                "The VHDX was attached but Windows did not expose a physical drive path."
                    .to_string(),
            );
        }
    };

    let disk_number = parse_disk_number(&device_path).ok_or_else(|| {
        format!("Attached VHDX returned an unrecognized device path: {device_path}")
    })?;

    info!(
        "Attached {} as {} (disk {})",
        abs_path.display(),
        device_path,
        disk_number
    );

    Ok(AttachedDiskInfo {
        disk_number,
        device_path,
        drive_letter: '\0',
        vhdx_path: abs_path,
    })
}

/// Detaches a VHDX. Already-detached images are treated as success.
pub fn detach_vhdx(image_path: &Path) -> Result<(), String> {
    if !image_path.exists() {
        return Ok(());
    }
    if !is_elevated() {
        return Err(
            "Detaching a virtual disk requires administrator privileges. Restart VirtualSSD as administrator."
                .to_string(),
        );
    }

    let abs_path = std::fs::canonicalize(image_path)
        .map_err(|e| format!("Failed to resolve VHDX path: {e}"))?;
    let wide_path = to_wide(&abs_path);
    let storage_type = vhdx_storage_type();

    let mut handle = HANDLE::default();
    let open_access = VIRTUAL_DISK_ACCESS_DETACH | VIRTUAL_DISK_ACCESS_GET_INFO;
    let result = unsafe {
        OpenVirtualDisk(
            &storage_type,
            PCWSTR(wide_path.as_ptr()),
            open_access,
            OPEN_VIRTUAL_DISK_FLAG_NONE,
            None,
            &mut handle,
        )
    };
    if result != ERROR_SUCCESS {
        if result == ERROR_FILE_NOT_FOUND || result == ERROR_NOT_FOUND {
            return Ok(());
        }
        return Err(describe_error("OpenVirtualDisk for detach", result));
    }

    let attached = unsafe { physical_path_for_handle(handle) }.is_ok();
    if !attached {
        let _ = unsafe { CloseHandle(handle) };
        return Ok(());
    }

    let result = unsafe { DetachVirtualDisk(handle, DETACH_VIRTUAL_DISK_FLAG_NONE, 0) };
    let _ = unsafe { CloseHandle(handle) };

    if result == ERROR_SUCCESS || result == ERROR_NOT_FOUND || result == ERROR_FILE_NOT_FOUND {
        info!("Detached {}", abs_path.display());
        Ok(())
    } else {
        Err(describe_error("DetachVirtualDisk", result))
    }
}

/// Formats an attached disk as NTFS and assigns a drive letter. The PowerShell
/// script is fixed; all values are passed through environment variables so no
/// caller-controlled text is ever interpolated into executable code.
pub fn format_volume(
    disk_number: u32,
    label: &str,
    preferred_letter: Option<char>,
) -> Result<char, String> {
    if !is_elevated() {
        return Err(
            "Formatting a virtual disk requires administrator privileges. Restart VirtualSSD as administrator."
                .to_string(),
        );
    }

    let letter = match preferred_letter {
        Some(letter) => validate_drive_letter(letter)?,
        None => choose_available_drive_letter()?,
    };
    let safe_label = sanitize_label(label)?;

    let script = FORMAT_VOLUME_SCRIPT;
    let output = run_powershell(
        script,
        &[
            ("VSSD_DISK_NUMBER", disk_number.to_string()),
            ("VSSD_DRIVE_LETTER", letter.to_string()),
            ("VSSD_VOLUME_LABEL", safe_label),
        ],
    )?;

    let assigned = output
        .trim()
        .lines()
        .last()
        .map(str::trim)
        .filter(|line| line.len() == 1)
        .and_then(|line| line.chars().next())
        .map(|c| c.to_ascii_uppercase())
        .ok_or_else(|| {
            format!("Windows did not report a drive letter after formatting: {output}")
        })?;

    if !assigned.is_ascii_uppercase() {
        return Err(format!(
            "Windows returned an invalid drive letter after formatting: {assigned}"
        ));
    }

    Ok(assigned)
}

/// Returns the VHDX path inside a disk directory.
pub fn vhdx_path_for_disk_dir(disk_dir: &Path) -> PathBuf {
    disk_dir.join(VHDX_FILE_NAME)
}

fn vhdx_storage_type() -> VIRTUAL_STORAGE_TYPE {
    VIRTUAL_STORAGE_TYPE {
        DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
        VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
    }
}

unsafe fn physical_path_for_handle(handle: HANDLE) -> Result<String, WIN32_ERROR> {
    let mut buffer = [0u16; 512];
    let mut size_bytes = (buffer.len() * std::mem::size_of::<u16>()) as u32;
    let result =
        unsafe { GetVirtualDiskPhysicalPath(handle, &mut size_bytes, PWSTR(buffer.as_mut_ptr())) };
    if result != ERROR_SUCCESS {
        return Err(result);
    }

    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..len]))
}

fn parse_disk_number(device_path: &str) -> Option<u32> {
    let digits: String = device_path
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    digits.parse().ok()
}

/// `FSCTL_FLUSH_VOLUME`.
///
/// The Windows SDK defines this as
/// `CTL_CODE(FILE_DEVICE_FILE_SYSTEM, 25, METHOD_BUFFERED, FILE_ANY_ACCESS)`,
/// which is `0x000A0064`. The `windows` crate does not expose it, so it is
/// defined here.
const FSCTL_FLUSH_VOLUME: u32 = 0x000A_0064;

/// Flushes the guest file system on a mounted volume down into the virtual
/// disk.
///
/// This is what makes a volume shadow copy of a *mounted* VHDX as fresh as
/// possible: NTFS writes everything the guest has committed out to the
/// virtual disk, and the VSS writer then flushes the file system cache
/// before the image is taken.
///
/// A failure is returned rather than swallowed. It is not fatal — the
/// snapshot can still be taken — but the caller has to know that the copy
/// might be slightly behind the live volume.
pub fn flush_volume(drive_letter: &str) -> Result<(), String> {
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let letter = drive_letter.trim().trim_end_matches(':');
    if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!("Invalid drive letter '{drive_letter}'"));
    }

    let path = format!("\\\\.\\{letter}:");
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        // GENERIC_READ | GENERIC_WRITE.
        const ACCESS: u32 = 0x8000_0000 | 0x4000_0000;

        let handle = CreateFileW(
            windows::core::PCWSTR(wide.as_ptr()),
            ACCESS,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
        .map_err(|e| format!("The volume {letter}: could not be opened for flushing: {e}"))?;

        let _guard = OwnedHandle(handle);

        let mut returned = 0u32;
        windows::Win32::System::IO::DeviceIoControl(
            handle,
            FSCTL_FLUSH_VOLUME,
            None,
            0,
            None,
            0,
            Some(&mut returned),
            None,
        )
        .map_err(|e| format!("The volume {letter}: could not be flushed: {e}"))?;
    }

    Ok(())
}

/// A Windows handle that is closed when it goes out of scope.
struct OwnedHandle(windows::Win32::Foundation::HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Validates and normalizes a drive letter. The system drive, letters in use,
/// and non A-Z input are rejected.
pub fn validate_drive_letter(letter: char) -> Result<char, String> {
    let letter = letter.to_ascii_uppercase();
    if !letter.is_ascii_uppercase() {
        return Err(format!("Invalid drive letter: {letter}"));
    }
    if letter == 'C' {
        return Err("Drive C: is the system drive and cannot be used.".to_string());
    }
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err("Windows could not enumerate logical drives.".to_string());
    }
    let index = (letter as u8 - b'A') as u32;
    if mask & (1u32 << index) != 0 {
        return Err(format!("Drive {letter}: is already in use."));
    }
    Ok(letter)
}

/// Picks the first free letter, preferring V: through Z: before D: through U:.
pub fn choose_available_drive_letter() -> Result<char, String> {
    let mask = unsafe { GetLogicalDrives() };
    if mask == 0 {
        return Err("Windows could not enumerate logical drives.".to_string());
    }

    let candidates = [
        'V', 'W', 'X', 'Y', 'Z', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P',
        'Q', 'R', 'S', 'T', 'U',
    ];
    candidates
        .into_iter()
        .find(|letter| {
            let index = (*letter as u8 - b'A') as u32;
            mask & (1u32 << index) == 0
        })
        .ok_or_else(|| "No free drive letters are available.".to_string())
}

fn sanitize_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() {
        return Ok("VirtualSSD".to_string());
    }
    if label.len() > 32 {
        return Err("Volume label cannot exceed 32 characters.".to_string());
    }
    if !label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_')
    {
        return Err(
            "Volume label may only contain letters, digits, spaces, hyphens, and underscores."
                .to_string(),
        );
    }
    Ok(label.to_string())
}

fn describe_error(operation: &str, code: WIN32_ERROR) -> String {
    if code == ERROR_ACCESS_DENIED {
        return format!(
            "{operation} failed with access denied. VirtualSSD must run as administrator to attach or format disks."
        );
    }
    let io = std::io::Error::from_raw_os_error(code.0 as i32);
    format!("{operation} failed (Win32 error {}: {io})", code.0)
}

fn to_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn run_powershell(script: &str, env: &[(&str, String)]) -> Result<String, String> {
    let encoded = encode_powershell(script);
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
        &encoded,
    ]);
    for (key, value) in env {
        command.env(key, value);
    }
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to start PowerShell: {e}"))?;

    let deadline = std::time::Instant::now() + POWER_SHELL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|e| format!("Failed to read PowerShell output: {e}"))?;
                if !status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    error!("PowerShell failed: stdout={stdout} stderr={stderr}");
                    return Err(format!(
                        "Windows volume preparation failed: {}",
                        if stderr.trim().is_empty() {
                            stdout.trim()
                        } else {
                            stderr.trim()
                        }
                    ));
                }
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err(
                        "Windows volume preparation timed out after 180 seconds.".to_string()
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Failed while waiting for PowerShell: {e}")),
        }
    }
}

fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64_encode(&bytes)
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let combined = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((combined >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((combined >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((combined >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(combined & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

const FORMAT_VOLUME_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$diskNumber = [uint32]$env:VSSD_DISK_NUMBER
$desiredLetter = $env:VSSD_DRIVE_LETTER
$label = $env:VSSD_VOLUME_LABEL

if ([string]::IsNullOrWhiteSpace($desiredLetter)) {
    throw 'No drive letter was supplied to the formatter.'
}
$desiredLetter = $desiredLetter.Trim().ToUpperInvariant()[0]

function Wait-ForDisk([uint32]$number) {
    for ($i = 0; $i -lt 40; $i++) {
        $candidate = Get-Disk -Number $number -ErrorAction SilentlyContinue
        if ($null -ne $candidate) { return $candidate }
        Start-Sleep -Milliseconds 250
    }
    throw "Disk $number did not appear within 10 seconds."
}

$disk = Wait-ForDisk $diskNumber
Set-Disk -Number $diskNumber -IsOffline $false -IsReadOnly $false -ErrorAction Stop

if ($disk.PartitionStyle -eq 'RAW') {
    Initialize-Disk -Number $diskNumber -PartitionStyle GPT -Confirm:$false -ErrorAction Stop | Out-Null
    $disk = Get-Disk -Number $diskNumber -ErrorAction Stop
}
elseif ($disk.PartitionStyle -ne 'GPT') {
    throw "Unsupported partition style '$($disk.PartitionStyle)' on disk $diskNumber. Refusing to modify it."
}

$partition = Get-Partition -DiskNumber $diskNumber -ErrorAction SilentlyContinue |
    Where-Object { $_.Type -eq 'Basic' } |
    Select-Object -First 1

if ($null -eq $partition) {
    $partition = New-Partition -DiskNumber $diskNumber -UseMaximumSize -DriveLetter $desiredLetter -ErrorAction Stop
}

$currentLetter = ([string]$partition.DriveLetter).Trim()
if ($currentLetter.Length -eq 0 -or ([int][char]$partition.DriveLetter) -eq 0) {
    Set-Partition -DiskNumber $diskNumber -PartitionNumber $partition.PartitionNumber -NewDriveLetter $desiredLetter -ErrorAction Stop
    $partition = Get-Partition -DiskNumber $diskNumber -PartitionNumber $partition.PartitionNumber -ErrorAction Stop
}

$volume = Get-Volume -Partition $partition -ErrorAction SilentlyContinue
if ($null -eq $volume) {
    Format-Volume -Partition $partition -FileSystem NTFS -NewFileSystemLabel $label -Confirm:$false -Force -ErrorAction Stop | Out-Null
}
elseif ([string]::IsNullOrWhiteSpace($volume.FileSystem)) {
    Format-Volume -Partition $partition -FileSystem NTFS -NewFileSystemLabel $label -Confirm:$false -Force -ErrorAction Stop | Out-Null
}
elseif ($volume.FileSystem -eq 'NTFS') {
    $drive = ([string]$partition.DriveLetter).Trim()
    if ($drive.Length -eq 1) {
        Set-Volume -DriveLetter $drive -NewFileSystemLabel $label -ErrorAction Stop
    }
}
else {
    throw "Existing $($volume.FileSystem) filesystem is not NTFS. Refusing to reformat an existing volume."
}

$finalPartition = Get-Partition -DiskNumber $diskNumber -ErrorAction Stop |
    Where-Object { $_.DriveLetter } |
    Select-Object -First 1
if ($null -eq $finalPartition) {
    throw 'Volume preparation completed but Windows did not assign a drive letter.'
}
$finalLetter = ([string]$finalPartition.DriveLetter).Trim()
if ($finalLetter.Length -ne 1) {
    throw "Unexpected drive letter '$finalLetter'."
}
Write-Output $finalLetter
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_physical_drive_paths() {
        assert_eq!(parse_disk_number(r"\\.\PhysicalDrive3"), Some(3));
        assert_eq!(parse_disk_number(r"\\?\PhysicalDrive12"), Some(12));
        assert_eq!(parse_disk_number("nonsense"), None);
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn label_validation_blocks_command_text() {
        assert!(sanitize_label("My Disk").is_ok());
        assert!(sanitize_label("My-Disk_1").is_ok());
        assert!(sanitize_label("x'; Remove-Item C:\\").is_err());
    }
}
