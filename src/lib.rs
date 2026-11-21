//! Conservative volume-to-physical-disk mapping for Alosh Engineering.
//!
//! This component deliberately reports all extents, not a single guessed disk:
//! Storage Spaces, RAID, dynamic volumes and spanned volumes may map to more
//! than one physical device. Callers must retain the full set.

#[path = "storage_probe.rs"]
pub mod storage_probe;

#[path = "device_profile.rs"]
pub mod device_profile;

#[path = "volume_inventory.rs"]
pub mod volume_inventory;

#[path = "io_scheduler.rs"]
pub mod io_scheduler;

#[path = "telemetry.rs"]
pub mod telemetry;

#[path = "topology_cache.rs"]
pub mod topology_cache;

#[path = "resumable_executor.rs"]
pub mod resumable_executor;

#[path = "sqlite_store.rs"]
pub mod sqlite_store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyStatus {
    Confirmed,
    Unsupported,
    AccessDenied,
    VolumeUnavailable,
    MalformedResponse,
    TooManyExtents,
    PlatformUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeDiskMapping {
    pub disk_numbers: Vec<u32>,
    pub status: TopologyStatus,
}

impl VolumeDiskMapping {
    pub fn unknown(status: TopologyStatus) -> Self {
        Self { disk_numbers: Vec::new(), status }
    }
}


/// Result of resolving an arbitrary filesystem path to its containing volume.
/// Paths are returned in Windows-native form and are not physical disk paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVolume {
    pub mount_path: String,
    pub volume_guid_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolveStatus {
    Confirmed,
    InvalidPath,
    NetworkOrRemoteVolume,
    AccessDenied,
    PathUnavailable,
    Unsupported,
    PlatformUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathResolveResult {
    pub volume: Option<ResolvedVolume>,
    pub status: PathResolveStatus,
}

impl PathResolveResult {
    pub fn unknown(status: PathResolveStatus) -> Self {
        Self { volume: None, status }
    }
}

/// Validate input before calling platform APIs. Device namespace paths are
/// intentionally rejected: this resolver accepts filesystem paths only.
pub fn validate_filesystem_path(path: &str) -> Result<(), PathResolveStatus> {
    if path.is_empty() || path.contains('\0') {
        return Err(PathResolveStatus::InvalidPath);
    }

    let lower = path.to_ascii_lowercase();
    if lower.starts_with(r"\\.\") {
        // Win32 device namespace (PhysicalDrive, pipe, device object, etc.).
        return Err(PathResolveStatus::InvalidPath);
    }
    if lower.starts_with(r"\\?\unc\")
        || (lower.starts_with(r"\\") && !lower.starts_with(r"\\?\"))
    {
        // UNC paths are remote shares, not local physical-volume mappings.
        return Err(PathResolveStatus::NetworkOrRemoteVolume);
    }

    // Extended-length DOS paths are valid filesystem paths. Accept only the
    // drive-root form here; do not accidentally admit GLOBALROOT/device paths.
    let (candidate, extended) = if lower.starts_with(r"\\?\") {
        (&path[4..], true)
    } else {
        (path, false)
    };
    let bytes = candidate.as_bytes();
    let valid_drive_root = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || (!extended && bytes[2] == b'/'));
    if !valid_drive_root {
        return Err(PathResolveStatus::InvalidPath);
    }
    Ok(())
}

/// Normalize only the two volume-device forms accepted by
/// FSCTL_GET_VOLUME_DISK_EXTENTS. Arbitrary Win32 device paths are rejected.
fn normalize_volume_device_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    if bytes.len() == 6
        && &bytes[..4] == br"\\.\"
        && bytes[4].is_ascii_alphabetic()
        && bytes[5] == b':'
    {
        return Some(path.to_owned());
    }
    volume_inventory::normalize_volume_guid_path(path)
}

const HEADER_SIZE: usize = 8;
const EXTENT_SIZE: usize = 24;
const MAX_EXTENTS: usize = 256;

/// Parse the documented x64/x86 Windows VOLUME_DISK_EXTENTS wire layout.
/// The caller must pass exactly the number of bytes reported by DeviceIoControl.
pub fn parse_volume_disk_extents(bytes: &[u8]) -> VolumeDiskMapping {
    if bytes.len() < HEADER_SIZE {
        return VolumeDiskMapping::unknown(TopologyStatus::MalformedResponse);
    }
    let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if count == 0 {
        return VolumeDiskMapping::unknown(TopologyStatus::MalformedResponse);
    }
    if count > MAX_EXTENTS {
        return VolumeDiskMapping::unknown(TopologyStatus::TooManyExtents);
    }
    let Some(required) = count.checked_mul(EXTENT_SIZE).and_then(|n| HEADER_SIZE.checked_add(n)) else {
        return VolumeDiskMapping::unknown(TopologyStatus::MalformedResponse);
    };
    if bytes.len() < required {
        return VolumeDiskMapping::unknown(TopologyStatus::MalformedResponse);
    }

    let mut disks = Vec::with_capacity(count);
    for index in 0..count {
        let offset = HEADER_SIZE + index * EXTENT_SIZE;
        let disk = u32::from_le_bytes([
            bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3],
        ]);
        if !disks.contains(&disk) {
            disks.push(disk);
        }
    }
    if disks.is_empty() {
        VolumeDiskMapping::unknown(TopologyStatus::MalformedResponse)
    } else {
        VolumeDiskMapping { disk_numbers: disks, status: TopologyStatus::Confirmed }
    }
}

#[cfg(windows)]
pub mod windows {
    use super::{normalize_volume_device_path, parse_volume_disk_extents, TopologyStatus, VolumeDiskMapping};
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    type Handle = *mut core::ffi::c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const FSCTL_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;
    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    const ERROR_PATH_NOT_FOUND: u32 = 3;
    const ERROR_MORE_DATA: u32 = 234;
    const MAX_BUFFER: usize = 1024 * 1024;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(name: *const u16, access: u32, share: u32,
            security: *const core::ffi::c_void, creation: u32, flags: u32, template: Handle) -> Handle;
        fn DeviceIoControl(handle: Handle, code: u32, in_buffer: *const core::ffi::c_void,
            in_size: u32, out_buffer: *mut core::ffi::c_void, out_size: u32,
            returned: *mut u32, overlapped: *mut core::ffi::c_void) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
    }

    const ERROR_INVALID_NAME: u32 = 123;
    const ERROR_BAD_NETPATH: u32 = 53;
    const ERROR_BAD_NET_NAME: u32 = 67;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetVolumePathNameW(file_name: *const u16, volume_path_name: *mut u16, length: u32) -> i32;
        fn GetVolumeNameForVolumeMountPointW(volume_mount_point: *const u16,
            volume_name: *mut u16, buffer_length: u32) -> i32;
    }

    /// Resolve a drive-letter or mounted-folder filesystem path to its volume
    /// mount root and volume GUID path. UNC shares are deliberately not mapped
    /// to local physical disks.
    pub fn resolve_path_to_volume(path: &str) -> super::PathResolveResult {
        if let Err(status) = super::validate_filesystem_path(path) {
            return super::PathResolveResult::unknown(status);
        }
        let input: Vec<u16> = OsStr::new(path).encode_wide().chain(once(0)).collect();
        let mut mount = vec![0u16; 32768];
        let ok = unsafe { GetVolumePathNameW(input.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) };
        if ok == 0 {
            let e = unsafe { GetLastError() };
            return super::PathResolveResult::unknown(match e {
                ERROR_ACCESS_DENIED => super::PathResolveStatus::AccessDenied,
                ERROR_INVALID_NAME | ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => super::PathResolveStatus::PathUnavailable,
                ERROR_BAD_NETPATH | ERROR_BAD_NET_NAME => super::PathResolveStatus::NetworkOrRemoteVolume,
                _ => super::PathResolveStatus::Unsupported,
            });
        }
        let mount_end = mount.iter().position(|c| *c == 0).unwrap_or(mount.len());
        if mount_end == 0 || mount_end >= mount.len() {
            return super::PathResolveResult::unknown(super::PathResolveStatus::Unsupported);
        }
        let mount_path = String::from_utf16_lossy(&mount[..mount_end]);
        let mount_wide: Vec<u16> = OsStr::new(&mount_path).encode_wide().chain(once(0)).collect();
        let mut guid = vec![0u16; 128];
        let guid_ok = unsafe { GetVolumeNameForVolumeMountPointW(mount_wide.as_ptr(), guid.as_mut_ptr(), guid.len() as u32) };
        if guid_ok == 0 {
            let e = unsafe { GetLastError() };
            return super::PathResolveResult::unknown(match e {
                ERROR_ACCESS_DENIED => super::PathResolveStatus::AccessDenied,
                ERROR_BAD_NETPATH | ERROR_BAD_NET_NAME => super::PathResolveStatus::NetworkOrRemoteVolume,
                _ => super::PathResolveStatus::Unsupported,
            });
        }
        let guid_end = guid.iter().position(|c| *c == 0).unwrap_or(guid.len());
        if guid_end == 0 || guid_end >= guid.len() {
            return super::PathResolveResult::unknown(super::PathResolveStatus::Unsupported);
        }
        let volume_guid_path = String::from_utf16_lossy(&guid[..guid_end]);
        super::PathResolveResult {
            volume: Some(super::ResolvedVolume { mount_path, volume_guid_path }),
            status: super::PathResolveStatus::Confirmed,
        }
    }

    /// Query a volume device path such as `\\.\C:`. Returns every distinct
    /// physical disk number backing the volume. Resolve a user path to its
    /// volume mount point before calling this function.
    pub fn query_volume_disk_extents(volume_device_path: &str) -> VolumeDiskMapping {
        let Some(device_path) = normalize_volume_device_path(volume_device_path) else {
            return VolumeDiskMapping::unknown(TopologyStatus::MalformedResponse);
        };
        let wide: Vec<u16> = OsStr::new(&device_path).encode_wide().chain(once(0)).collect();
        let handle = unsafe { CreateFileW(wide.as_ptr(), GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL, null_mut()) };
        if handle == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };
            return VolumeDiskMapping::unknown(match error {
                ERROR_ACCESS_DENIED => TopologyStatus::AccessDenied,
                ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => TopologyStatus::VolumeUnavailable,
                _ => TopologyStatus::Unsupported,
            });
        }

        let mut capacity = 4096usize;
        let result = loop {
            let mut buffer = vec![0u8; capacity];
            let mut returned = 0u32;
            let ok = unsafe { DeviceIoControl(handle, FSCTL_GET_VOLUME_DISK_EXTENTS,
                null(), 0, buffer.as_mut_ptr() as *mut core::ffi::c_void,
                buffer.len() as u32, &mut returned, null_mut()) };
            if ok != 0 {
                if returned as usize > buffer.len() {
                    break VolumeDiskMapping::unknown(TopologyStatus::MalformedResponse);
                }
                break parse_volume_disk_extents(&buffer[..returned as usize]);
            }
            let error = unsafe { GetLastError() };
            if error == ERROR_MORE_DATA && capacity < MAX_BUFFER {
                capacity = (capacity * 2).min(MAX_BUFFER);
                continue;
            }
            break VolumeDiskMapping::unknown(if error == ERROR_MORE_DATA {
                TopologyStatus::TooManyExtents
            } else if error == ERROR_ACCESS_DENIED {
                TopologyStatus::AccessDenied
            } else {
                TopologyStatus::Unsupported
            });
        };
        unsafe { CloseHandle(handle); }
        result
    }
}

#[cfg(not(windows))]
pub mod windows {
    use super::{PathResolveResult, PathResolveStatus, TopologyStatus, VolumeDiskMapping};
    pub fn resolve_path_to_volume(_path: &str) -> PathResolveResult {
        PathResolveResult::unknown(PathResolveStatus::PlatformUnsupported)
    }
    pub fn query_volume_disk_extents(_volume_device_path: &str) -> VolumeDiskMapping {
        VolumeDiskMapping::unknown(TopologyStatus::PlatformUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extent(disk: u32) -> [u8; EXTENT_SIZE] {
        let mut bytes = [0u8; EXTENT_SIZE];
        bytes[..4].copy_from_slice(&disk.to_le_bytes());
        bytes
    }

    fn payload(disks: &[u32]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_SIZE];
        out[..4].copy_from_slice(&(disks.len() as u32).to_le_bytes());
        for disk in disks { out.extend_from_slice(&extent(*disk)); }
        out
    }

    #[test]
    fn maps_single_disk_volume() {
        let m = parse_volume_disk_extents(&payload(&[3]));
        assert_eq!(m.disk_numbers, vec![3]);
        assert_eq!(m.status, TopologyStatus::Confirmed);
    }

    #[test]
    fn preserves_multiple_physical_disks() {
        let m = parse_volume_disk_extents(&payload(&[2, 5]));
        assert_eq!(m.disk_numbers, vec![2, 5]);
    }

    #[test]
    fn deduplicates_repeated_disk_extents() {
        let m = parse_volume_disk_extents(&payload(&[1, 1, 4]));
        assert_eq!(m.disk_numbers, vec![1, 4]);
    }

    #[test]
    fn rejects_truncated_response() {
        let mut bytes = payload(&[0]);
        bytes.pop();
        assert_eq!(parse_volume_disk_extents(&bytes).status, TopologyStatus::MalformedResponse);
    }

    #[test]
    fn rejects_zero_extents() {
        assert_eq!(parse_volume_disk_extents(&payload(&[])).status, TopologyStatus::MalformedResponse);
    }

    #[test]
    fn accepts_absolute_drive_paths() {
        assert_eq!(validate_filesystem_path(r"C:\Users\Public\file.txt"), Ok(()));
    }

    #[test]
    fn accepts_forward_slash_drive_paths() {
        assert_eq!(validate_filesystem_path("D:/Data/photo.jpg"), Ok(()));
    }

    #[test]
    fn rejects_relative_and_drive_rootless_paths() {
        assert_eq!(validate_filesystem_path("folder\\file.txt"), Err(PathResolveStatus::InvalidPath));
        assert_eq!(validate_filesystem_path("C:file.txt"), Err(PathResolveStatus::InvalidPath));
    }

    #[test]
    fn rejects_device_namespace_paths() {
        assert_eq!(validate_filesystem_path(r"\\.\PhysicalDrive0"), Err(PathResolveStatus::InvalidPath));
        assert_eq!(validate_filesystem_path(r"\\?\GLOBALROOT\Device\Harddisk0"), Err(PathResolveStatus::InvalidPath));
    }

    #[test]
    fn keeps_unc_paths_out_of_local_disk_mapping() {
        assert_eq!(validate_filesystem_path(r"\\server\share\file.txt"), Err(PathResolveStatus::NetworkOrRemoteVolume));
    }

    #[test]
    fn accepts_extended_length_drive_paths_but_not_device_namespace_paths() {
        assert_eq!(validate_filesystem_path(r"\\?\C:\very\long\path.bin"), Ok(()));
        assert_eq!(validate_filesystem_path(r"\\?\GLOBALROOT\Device\Harddisk0"), Err(PathResolveStatus::InvalidPath));
        assert_eq!(validate_filesystem_path(r"\\?\UNC\server\share\file"), Err(PathResolveStatus::NetworkOrRemoteVolume));
    }

    #[test]
    fn volume_extent_device_path_validation_is_exact() {
        assert_eq!(normalize_volume_device_path(r"\\.\D:").as_deref(), Some(r"\\.\D:"));
        assert!(normalize_volume_device_path(r"\\.\PhysicalDrive0").is_none());
        assert!(normalize_volume_device_path(r"\\.\C:\folder").is_none());
        assert_eq!(normalize_volume_device_path(r"\\?\volume{01234567-89ab-cdef-0123-456789abcdef}\").as_deref(), Some(r"\\?\Volume{01234567-89AB-CDEF-0123-456789ABCDEF}"));
    }

    #[test]
    fn rejects_empty_and_nul_paths() {
        assert_eq!(validate_filesystem_path("  "), Err(PathResolveStatus::InvalidPath));
        assert_eq!(validate_filesystem_path("C:\\bad\\0name"), Ok(()));
        assert_eq!(validate_filesystem_path("C:\\bad\0name"), Err(PathResolveStatus::InvalidPath));
    }

    #[test]
    fn caps_unreasonable_extent_count() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[..4].copy_from_slice(&257u32.to_le_bytes());
        assert_eq!(parse_volume_disk_extents(&bytes).status, TopologyStatus::TooManyExtents);
    }
}
