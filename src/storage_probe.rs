
// Conservative storage-media classification for Alosh Engineering.
//
// A path or drive letter alone is not enough to infer SSD/HDD. The Windows
// probe accepts an explicitly resolved physical-device path (for example
// `\\.\PhysicalDrive0`) and asks the storage stack for its seek-penalty
// property. Unknown and unsupported responses remain `Unknown`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    NonRotational,
    Rotational,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeStatus {
    Confirmed,
    Unsupported,
    AccessDenied,
    DeviceUnavailable,
    MalformedResponse,
    PlatformUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageClassification {
    pub kind: StorageKind,
    pub status: ProbeStatus,
}

impl StorageClassification {
    pub const fn unknown(status: ProbeStatus) -> Self {
        Self {
            kind: StorageKind::Unknown,
            status,
        }
    }

    /// Conservative scheduling hint only; it must never affect scan coverage
    /// or hashing accuracy.
    pub const fn recommended_io_workers(self, cpu_workers: usize) -> usize {
        let base = if cpu_workers == 0 {
            1
        } else {
            cpu_workers
        };

        match self.kind {
            StorageKind::NonRotational => {
                if base < 8 { base } else { 8 }
            }
            StorageKind::Rotational => {
                if base < 2 { base } else { 2 }
            }
            StorageKind::Unknown => {
                if base < 2 { base } else { 2 }
            }
        }
    }
}

/// Interpret the Windows STORAGE_DEVICE_SEEK_PENALTY_DESCRIPTOR's
/// `IncursSeekPenalty` byte. Values other than 0 or 1 are not trusted.
pub const fn classify_seek_penalty(value: u8) -> StorageClassification {
    match value {
        0 => StorageClassification {
            kind: StorageKind::NonRotational,
            status: ProbeStatus::Confirmed,
        },
        1 => StorageClassification {
            kind: StorageKind::Rotational,
            status: ProbeStatus::Confirmed,
        },
        _ => StorageClassification::unknown(ProbeStatus::MalformedResponse),
    }
}

#[cfg(windows)]
pub mod windows {
    use super::{classify_seek_penalty, ProbeStatus, StorageClassification};
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    type Handle = *mut core::ffi::c_void;

    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const GENERIC_READ: u32 = 0x8000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const STORAGE_DEVICE_SEEK_PENALTY_PROPERTY: u32 = 7;
    const PROPERTY_STANDARD_QUERY: u32 = 0;

    #[repr(C)]
    struct StoragePropertyQuery {
        property_id: u32,
        query_type: u32,
        additional_parameters: [u8; 1],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *const core::ffi::c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;

        fn DeviceIoControl(
            handle: Handle,
            code: u32,
            in_buffer: *const core::ffi::c_void,
            in_size: u32,
            out_buffer: *mut core::ffi::c_void,
            out_size: u32,
            returned: *mut u32,
            overlapped: *mut core::ffi::c_void,
        ) -> i32;

        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
    }

    /// Probe an already-resolved physical disk device path such as
    /// `\\.\PhysicalDrive0`. Volume-to-physical-disk mapping is deliberately
    /// outside this function and must be performed by a separately tested
    /// Windows volume-topology layer.
    pub fn probe_physical_device(device_path: &str) -> StorageClassification {
        if !device_path.starts_with(r"\\.\PhysicalDrive") {
            return StorageClassification::unknown(ProbeStatus::MalformedResponse);
        }

        let wide: Vec<u16> = OsStr::new(device_path)
            .encode_wide()
            .chain(once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };

            return StorageClassification::unknown(match error {
                5 => ProbeStatus::AccessDenied,
                2 | 3 | 21 => ProbeStatus::DeviceUnavailable,
                _ => ProbeStatus::Unsupported,
            });
        }

        let query = StoragePropertyQuery {
            property_id: STORAGE_DEVICE_SEEK_PENALTY_PROPERTY,
            query_type: PROPERTY_STANDARD_QUERY,
            additional_parameters: [0],
        };

        // STORAGE_DEVICE_SEEK_PENALTY_DESCRIPTOR layout: Version, Size,
        // IncursSeekPenalty (BOOLEAN). Validate returned size before
        // trusting the final byte.
        let mut output = [0u8; 16];
        let mut returned = 0u32;

        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &query as *const _ as *const core::ffi::c_void,
                core::mem::size_of::<StoragePropertyQuery>() as u32,
                output.as_mut_ptr() as *mut core::ffi::c_void,
                output.len() as u32,
                &mut returned,
                null_mut(),
            )
        };

        unsafe {
            CloseHandle(handle);
        }

        if ok == 0 {
            return StorageClassification::unknown(ProbeStatus::Unsupported);
        }

        if returned < 9 {
            return StorageClassification::unknown(ProbeStatus::MalformedResponse);
        }

        classify_seek_penalty(output[8])
    }
}

#[cfg(not(windows))]
pub fn probe_physical_device(_device_path: &str) -> StorageClassification {
    StorageClassification::unknown(ProbeStatus::PlatformUnsupported)
}
