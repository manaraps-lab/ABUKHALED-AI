//! Confirmed-only, Volume-GUID-keyed storage topology cache.
//!
//! Cache invalidation is driven only by a confirmed complete inventory diff.
//! An enumeration failure never clears the cache or implies device removal.

use std::collections::BTreeMap;

use crate::device_profile::{ProfileStatus, VolumeStorageProfile};
use crate::{TopologyStatus, VolumeDiskMapping};
use crate::volume_inventory::{normalize_volume_guid_path, VolumeChangeStatus, VolumeSetDiff};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedVolumeTopology {
    pub mapping: VolumeDiskMapping,
    pub profile: VolumeStorageProfile,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInsertStatus {
    Inserted,
    InvalidVolumeGuid,
    UnconfirmedMapping,
    UnconfirmedProfile,
    MappingProfileMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInvalidationStatus {
    Applied,
    IgnoredUnconfirmedDiff,
    IgnoredInvalidDiff,
    GenerationExhausted,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TopologyCache {
    generation: u64,
    entries: BTreeMap<String, CachedVolumeTopology>,
}

impl TopologyCache {
    pub const fn generation(&self) -> u64 { self.generation }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    pub fn get(&self, volume_guid: &str) -> Option<&CachedVolumeTopology> {
        let key = normalize_volume_guid_path(volume_guid)?;
        self.entries.get(&key).filter(|entry| entry.generation == self.generation)
    }

    /// Only complete, mutually consistent mapping/profile pairs are cached.
    /// Unknown or partial results remain diagnostics, never reusable topology.
    pub fn insert(
        &mut self,
        volume_guid: &str,
        mapping: VolumeDiskMapping,
        profile: VolumeStorageProfile,
    ) -> CacheInsertStatus {
        let Some(key) = normalize_volume_guid_path(volume_guid) else {
            return CacheInsertStatus::InvalidVolumeGuid;
        };
        if mapping.status != TopologyStatus::Confirmed || mapping.disk_numbers.is_empty() {
            return CacheInsertStatus::UnconfirmedMapping;
        }
        if profile.status != ProfileStatus::Confirmed || profile.devices.is_empty() {
            return CacheInsertStatus::UnconfirmedProfile;
        }
        let mut mapped = mapping.disk_numbers.clone();
        let mut profiled: Vec<u32> = profile.devices.iter().map(|d| d.disk_number).collect();
        mapped.sort_unstable(); mapped.dedup();
        profiled.sort_unstable(); profiled.dedup();
        if mapped != profiled || mapped.len() != mapping.disk_numbers.len() || profiled.len() != profile.devices.len() {
            return CacheInsertStatus::MappingProfileMismatch;
        }
        self.entries.insert(key, CachedVolumeTopology {
            mapping, profile, generation: self.generation,
        });
        CacheInsertStatus::Inserted
    }

    /// Apply only an authoritative complete diff. Changed volumes are evicted;
    /// unaffected entries are advanced to the new generation atomically.
    pub fn invalidate_from_diff(&mut self, diff: &VolumeSetDiff) -> CacheInvalidationStatus {
        if diff.status != VolumeChangeStatus::Confirmed {
            return CacheInvalidationStatus::IgnoredUnconfirmedDiff;
        }
        if diff.added.iter().chain(diff.removed.iter())
            .any(|path| normalize_volume_guid_path(path).is_none())
        {
            return CacheInvalidationStatus::IgnoredInvalidDiff;
        }
        if diff.added.is_empty() && diff.removed.is_empty() {
            return CacheInvalidationStatus::Applied;
        }
        let Some(next_generation) = self.generation.checked_add(1) else {
            return CacheInvalidationStatus::GenerationExhausted;
        };
        for path in diff.added.iter().chain(diff.removed.iter()) {
            if let Some(key) = normalize_volume_guid_path(path) { self.entries.remove(&key); }
        }
        self.generation = next_generation;
        for entry in self.entries.values_mut() { entry.generation = next_generation; }
        CacheInvalidationStatus::Applied
    }

    /// Clear all entries after an explicit host-level trust reset (for example,
    /// resume from hibernation when the host cannot establish continuity).
    pub fn clear(&mut self) -> CacheInvalidationStatus {
        let Some(next_generation) = self.generation.checked_add(1) else {
            return CacheInvalidationStatus::GenerationExhausted;
        };
        self.entries.clear();
        self.generation = next_generation;
        CacheInvalidationStatus::Applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_profile::{PhysicalDeviceProfile, VolumeMediaKind};
    use crate::storage_probe::{ProbeStatus, StorageClassification, StorageKind};
    use crate::volume_inventory::{VolumeChangeStatus, VolumeSetDiff};

    const A: &str = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\";
    const B: &str = r"\\?\Volume{11234567-89ab-cdef-0123-456789abcdef}\";

    fn confirmed_pair() -> (VolumeDiskMapping, VolumeStorageProfile) {
        let mapping = VolumeDiskMapping { disk_numbers: vec![0], status: TopologyStatus::Confirmed };
        let classification = StorageClassification { kind: StorageKind::NonRotational, status: ProbeStatus::Confirmed };
        let profile = VolumeStorageProfile { devices: vec![PhysicalDeviceProfile {
            disk_number: 0, device_path: r"\\.\PhysicalDrive0".into(), classification,
        }], kind: VolumeMediaKind::NonRotational, status: ProfileStatus::Confirmed };
        (mapping, profile)
    }

    #[test]
    fn inserts_and_reads_confirmed_topology() {
        let (m,p)=confirmed_pair(); let mut c=TopologyCache::default();
        assert_eq!(c.insert(A,m,p), CacheInsertStatus::Inserted);
        assert!(c.get(A).is_some());
    }
    #[test]
    fn refuses_unconfirmed_mapping_or_profile() {
        let (mut m,p)=confirmed_pair(); let mut c=TopologyCache::default();
        m.status=TopologyStatus::Unsupported;
        assert_eq!(c.insert(A,m,p), CacheInsertStatus::UnconfirmedMapping);
        assert!(c.is_empty());
    }
    #[test]
    fn refuses_mismatched_disk_sets() {
        let (m,mut p)=confirmed_pair(); p.devices[0].disk_number=3;
        let mut c=TopologyCache::default();
        assert_eq!(c.insert(A,m,p), CacheInsertStatus::MappingProfileMismatch);
    }
    #[test]
    fn unconfirmed_diff_never_invalidates_cache() {
        let (m,p)=confirmed_pair(); let mut c=TopologyCache::default(); c.insert(A,m,p);
        let diff=VolumeSetDiff { added: vec![], removed: vec![A.into()], status: VolumeChangeStatus::CannotCompareUnconfirmedSnapshot };
        assert_eq!(c.invalidate_from_diff(&diff), CacheInvalidationStatus::IgnoredUnconfirmedDiff);
        assert!(c.get(A).is_some());
    }
    #[test]
    fn confirmed_removal_evicts_only_changed_volume() {
        let (m,p)=confirmed_pair(); let mut c=TopologyCache::default();
        c.insert(A,m.clone(),p.clone()); c.insert(B,m,p);
        let diff=VolumeSetDiff { added: vec![], removed: vec![A.into()], status: VolumeChangeStatus::Confirmed };
        assert_eq!(c.invalidate_from_diff(&diff), CacheInvalidationStatus::Applied);
        assert!(c.get(A).is_none()); assert!(c.get(B).is_some());
        assert_eq!(c.generation(),1);
    }
    #[test]
    fn malformed_confirmed_diff_is_rejected_without_generation_change() {
        let (m,p)=confirmed_pair();
        let mut c=TopologyCache::default();
        c.insert(A,m,p);
        let diff=VolumeSetDiff { added: vec![], removed: vec!["not-a-volume".into()], status: VolumeChangeStatus::Confirmed };
        assert_eq!(c.invalidate_from_diff(&diff), CacheInvalidationStatus::IgnoredInvalidDiff);
        assert_eq!(c.generation(), 0);
        assert!(c.get(A).is_some());
    }

    #[test]
    fn malformed_changed_path_does_not_remove_unrelated_entry() {
        let (m,p)=confirmed_pair(); let mut c=TopologyCache::default(); c.insert(A,m,p);
        let diff=VolumeSetDiff { added: vec![], removed: vec!["not-a-volume".into()], status: VolumeChangeStatus::Confirmed };
        c.invalidate_from_diff(&diff);
        assert!(c.get(A).is_some());
    }
}
