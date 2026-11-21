
// Conservative aggregation of the physical devices backing a volume.
//
// Disk numbers originate from FSCTL_GET_VOLUME_DISK_EXTENTS. They are mapped
// to Windows physical-drive device paths without collapsing multi-disk sets.
// Any failed member probe makes the whole profile Unknown; partial knowledge
// is retained for diagnostics but never used to claim a homogeneous device.

use crate::storage_probe::{ProbeStatus, StorageClassification, StorageKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeMediaKind {
    NonRotational,
    Rotational,
    Mixed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileStatus {
    Confirmed,
    EmptyTopology,
    PartialProbeFailure,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalDeviceProfile {
    pub disk_number: u32,
    pub device_path: String,
    pub classification: StorageClassification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeStorageProfile {
    pub devices: Vec<PhysicalDeviceProfile>,
    pub kind: VolumeMediaKind,
    pub status: ProfileStatus,
}

impl VolumeStorageProfile {
    pub fn from_classifications(items: Vec<(u32, StorageClassification)>) -> Self {
        if items.is_empty() {
            return Self {
                devices: Vec::new(),
                kind: VolumeMediaKind::Unknown,
                status: ProfileStatus::EmptyTopology,
            };
        }

        let mut devices = Vec::with_capacity(items.len());
        let mut saw_nonrotational = false;
        let mut saw_rotational = false;
        let mut failed = false;

        for (disk_number, classification) in items {
            let device_path = physical_drive_path(disk_number);

            match (classification.status, classification.kind) {
                (ProbeStatus::Confirmed, StorageKind::NonRotational) => {
                    saw_nonrotational = true;
                }
                (ProbeStatus::Confirmed, StorageKind::Rotational) => {
                    saw_rotational = true;
                }
                _ => {
                    failed = true;
                }
            }

            devices.push(PhysicalDeviceProfile {
                disk_number,
                device_path,
                classification,
            });
        }

        // A failed probe prevents a confident aggregate classification, even
        // when all successfully queried members happen to agree.
        let (kind, status) = if failed {
            (
                VolumeMediaKind::Unknown,
                ProfileStatus::PartialProbeFailure,
            )
        } else if saw_nonrotational && saw_rotational {
            (
                VolumeMediaKind::Mixed,
                ProfileStatus::Confirmed,
            )
        } else if saw_nonrotational {
            (
                VolumeMediaKind::NonRotational,
                ProfileStatus::Confirmed,
            )
        } else if saw_rotational {
            (
                VolumeMediaKind::Rotational,
                ProfileStatus::Confirmed,
            )
        } else {
            (
                VolumeMediaKind::Unknown,
                ProfileStatus::PartialProbeFailure,
            )
        };

        Self {
            devices,
            kind,
            status,
        }
    }

    /// Scheduling hint only. It must never reduce scan coverage or accuracy.
    pub const fn recommended_io_workers(&self, cpu_workers: usize) -> usize {
        let cpu = if cpu_workers == 0 {
            1
        } else {
            cpu_workers
        };

        match self.kind {
            VolumeMediaKind::NonRotational => {
                if cpu < 8 { cpu } else { 8 }
            }
            VolumeMediaKind::Rotational | VolumeMediaKind::Mixed => {
                if cpu < 2 { cpu } else { 2 }
            }
            VolumeMediaKind::Unknown => 1,
        }
    }
}

/// Construct the documented Windows physical-drive path for a disk number.
pub fn physical_drive_path(disk_number: u32) -> String {
    format!(r"\\.\PhysicalDrive{disk_number}")
}

/// Validate a physical-drive path before passing it to a Windows API.
pub fn parse_physical_drive_number(path: &str) -> Option<u32> {
    let suffix = path.strip_prefix(r"\\.\PhysicalDrive")?;

    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    suffix.parse().ok()
}

#[cfg(windows)]
pub mod windows {
    use super::{parse_physical_drive_number, physical_drive_path, VolumeStorageProfile};
    use crate::{storage_probe, TopologyStatus, VolumeDiskMapping};

    /// Probe every distinct disk reported for a volume. The caller must pass
    /// the complete extent mapping; an unknown/empty mapping is not guessed.
    pub fn profile_volume(mapping: &VolumeDiskMapping) -> VolumeStorageProfile {
        if mapping.status != TopologyStatus::Confirmed || mapping.disk_numbers.is_empty() {
            return VolumeStorageProfile::from_classifications(Vec::new());
        }

        let mut results = Vec::with_capacity(mapping.disk_numbers.len());

        for &disk in &mapping.disk_numbers {
            let path = physical_drive_path(disk);

            // Defensive round-trip validation guards future path changes.
            if parse_physical_drive_number(&path) != Some(disk) {
                continue;
            }

            let classification = storage_probe::windows::probe_physical_device(&path);
            results.push((disk, classification));
        }

        VolumeStorageProfile::from_classifications(results)
    }
}

#[cfg(not(windows))]
pub mod windows {
    use super::VolumeStorageProfile;
    use crate::VolumeDiskMapping;

    pub fn profile_volume(_mapping: &VolumeDiskMapping) -> VolumeStorageProfile {
        VolumeStorageProfile::from_classifications(Vec::new())
    }
}
