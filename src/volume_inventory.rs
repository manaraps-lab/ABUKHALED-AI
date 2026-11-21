//! Snapshot-based volume inventory and conservative change detection.
//!
//! This module deliberately exposes polling primitives rather than spawning a
//! hidden background thread. The host application controls polling cadence and
//! must invalidate cached topology whenever a confirmed snapshot changes.

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryStatus {
    Confirmed,
    EnumerationFailed,
    PlatformUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeSetSnapshot {
    /// Canonical volume GUID paths without a trailing slash, sorted and unique.
    pub volume_paths: Vec<String>,
    pub status: InventoryStatus,
}

impl VolumeSetSnapshot {
    pub fn confirmed<I, S>(paths: I) -> Self
    where I: IntoIterator<Item = S>, S: AsRef<str> {
        let mut set = BTreeSet::new();
        for path in paths {
            let Some(value) = normalize_volume_guid_path(path.as_ref()) else {
                // A partially parsed enumeration must never be promoted to a
                // complete snapshot: doing so could manufacture false removals.
                return Self::unknown(InventoryStatus::EnumerationFailed);
            };
            set.insert(value);
        }
        Self { volume_paths: set.into_iter().collect(), status: InventoryStatus::Confirmed }
    }

    pub fn unknown(status: InventoryStatus) -> Self {
        Self { volume_paths: Vec::new(), status }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeChangeStatus {
    Confirmed,
    CannotCompareUnconfirmedSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeSetDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub status: VolumeChangeStatus,
}

impl VolumeSetDiff {
    pub fn cache_invalidation_required(&self) -> bool {
        self.status == VolumeChangeStatus::Confirmed
            && self.added.iter().chain(self.removed.iter())
                .all(|path| normalize_volume_guid_path(path).is_some())
            && (!self.added.is_empty() || !self.removed.is_empty())
    }
}

/// Compare two complete inventories. Never interpret an incomplete current or
/// previous inventory as evidence that a volume was removed.
pub fn diff_volume_sets(previous: &VolumeSetSnapshot, current: &VolumeSetSnapshot) -> VolumeSetDiff {
    // The snapshot fields are public for serialization/integration, so do not
    // assume every caller used `VolumeSetSnapshot::confirmed`. Revalidate the
    // complete contents before deriving additions/removals.
    let normalize_snapshot = |snapshot: &VolumeSetSnapshot| -> Option<BTreeSet<String>> {
        if snapshot.status != InventoryStatus::Confirmed {
            return None;
        }
        snapshot.volume_paths.iter()
            .map(|path| normalize_volume_guid_path(path))
            .collect()
    };
    let (Some(old), Some(new)) = (normalize_snapshot(previous), normalize_snapshot(current)) else {
        return VolumeSetDiff {
            added: Vec::new(), removed: Vec::new(),
            status: VolumeChangeStatus::CannotCompareUnconfirmedSnapshot,
        };
    };
    VolumeSetDiff {
        added: new.difference(&old).cloned().collect(),
        removed: old.difference(&new).cloned().collect(),
        status: VolumeChangeStatus::Confirmed,
    }
}

/// Return a canonical volume GUID path, or None for anything that is not a
/// volume GUID path. This intentionally does not accept drive letters/UNC.
pub fn normalize_volume_guid_path(path: &str) -> Option<String> {
    // Do not trim whitespace: silently repairing malformed API output could
    // turn an invalid entry into a trusted volume identity.
    let trimmed = path.trim_end_matches('\\');
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with(r"\\?\volume{") || !lower.ends_with('}') { return None; }
    let guid = &trimmed[r"\\?\Volume{".len()..trimmed.len() - 1];
    if guid.len() != 36 || !guid.bytes().enumerate().all(|(i, b)| {
        if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
    }) { return None; }
    Some(format!(r"\\?\Volume{{{}}}", guid.to_ascii_uppercase()))
}

#[cfg(windows)]
pub mod windows {
    use super::{InventoryStatus, VolumeSetSnapshot};

    type Handle = *mut core::ffi::c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const ERROR_NO_MORE_FILES: u32 = 18;

    #[link(name = "kernel32")]
    extern "system" {
        fn FindFirstVolumeW(volume_name: *mut u16, buffer_length: u32) -> Handle;
        fn FindNextVolumeW(find_volume: Handle, volume_name: *mut u16, buffer_length: u32) -> i32;
        fn FindVolumeClose(find_volume: Handle) -> i32;
        fn GetLastError() -> u32;
    }

    /// Enumerate the current set of local/available volume GUID paths.
    /// An enumeration error returns an unconfirmed snapshot, never an empty
    /// confirmed list (which could otherwise look like mass deletion).
    pub fn enumerate_volumes() -> VolumeSetSnapshot {
        let mut buffer = vec![0u16; 1024];
        let first = unsafe { FindFirstVolumeW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if first == INVALID_HANDLE_VALUE {
            return VolumeSetSnapshot::unknown(InventoryStatus::EnumerationFailed);
        }
        let mut paths = Vec::new();
        let mut failed = false;
        loop {
            let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
            if end == 0 || end == buffer.len() { failed = true; break; }
            paths.push(String::from_utf16_lossy(&buffer[..end]));
            buffer.fill(0);
            let ok = unsafe { FindNextVolumeW(first, buffer.as_mut_ptr(), buffer.len() as u32) };
            if ok == 0 {
                let error = unsafe { GetLastError() };
                if error != ERROR_NO_MORE_FILES { failed = true; }
                break;
            }
        }
        unsafe { FindVolumeClose(first); }
        if failed { VolumeSetSnapshot::unknown(InventoryStatus::EnumerationFailed) }
        else { VolumeSetSnapshot::confirmed(paths) }
    }

    /// One polling tick. The caller should schedule this at a modest interval
    /// and persist the last confirmed snapshot. No internal timer/thread exists.
    pub fn poll_volume_changes(previous: &VolumeSetSnapshot) -> (VolumeSetSnapshot, super::VolumeSetDiff) {
        let current = enumerate_volumes();
        let diff = super::diff_volume_sets(previous, &current);
        (current, diff)
    }
}

#[cfg(not(windows))]
pub mod windows {
    use super::{InventoryStatus, VolumeSetSnapshot};
    pub fn enumerate_volumes() -> VolumeSetSnapshot {
        VolumeSetSnapshot::unknown(InventoryStatus::PlatformUnsupported)
    }
    pub fn poll_volume_changes(previous: &VolumeSetSnapshot) -> (VolumeSetSnapshot, super::VolumeSetDiff) {
        let current = enumerate_volumes();
        let diff = super::diff_volume_sets(previous, &current);
        (current, diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const A: &str = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\";
    const B: &str = r"\\?\Volume{11234567-89ab-cdef-0123-456789abcdef}\";

    #[test]
    fn normalizes_guid_paths_case_and_trailing_separator() {
        assert_eq!(normalize_volume_guid_path(A).unwrap(), r"\\?\Volume{01234567-89AB-CDEF-0123-456789ABCDEF}");
    }
    #[test]
    fn rejects_non_guid_paths() {
        assert_eq!(normalize_volume_guid_path(r"C:\"), None);
        assert_eq!(normalize_volume_guid_path(r"\\server\share"), None);
        assert_eq!(normalize_volume_guid_path(r"\\?\Volume{bad}"), None);
    }
    #[test]
    fn malformed_entry_makes_the_entire_inventory_unconfirmed() {
        let snapshot = VolumeSetSnapshot::confirmed([A, "not-a-volume"]);
        assert_eq!(snapshot.status, InventoryStatus::EnumerationFailed);
        assert!(snapshot.volume_paths.is_empty());
    }

    #[test]
    fn whitespace_is_not_silently_normalized_into_a_volume_guid() {
        assert_eq!(normalize_volume_guid_path(&format!(" {}", A)), None);
        assert_eq!(normalize_volume_guid_path(&format!("{} ", A)), None);
    }

    #[test]
    fn deduplicates_and_sorts_confirmed_snapshot() {
        let s = VolumeSetSnapshot::confirmed([B, A, A]);
        assert_eq!(s.volume_paths.len(), 2);
        assert!(s.volume_paths[0] < s.volume_paths[1]);
    }
    #[test]
    fn detects_added_and_removed_volumes() {
        let old = VolumeSetSnapshot::confirmed([A]);
        let new = VolumeSetSnapshot::confirmed([B]);
        let d = diff_volume_sets(&old, &new);
        assert_eq!(d.added.len(), 1);
        assert_eq!(d.removed.len(), 1);
        assert!(d.cache_invalidation_required());
    }
    #[test]
    fn unchanged_inventory_does_not_invalidate_cache() {
        let a = VolumeSetSnapshot::confirmed([A, B]);
        let d = diff_volume_sets(&a, &a);
        assert!(!d.cache_invalidation_required());
    }
    #[test]
    fn malformed_confirmed_diff_does_not_request_cache_invalidation() {
        let diff = VolumeSetDiff {
            added: Vec::new(),
            removed: vec!["not-a-volume".to_owned()],
            status: VolumeChangeStatus::Confirmed,
        };
        assert!(!diff.cache_invalidation_required());
    }

    #[test]
    fn manually_constructed_confirmed_snapshot_with_invalid_path_is_not_trusted() {
        let previous = VolumeSetSnapshot::confirmed([A, B]);
        let current = VolumeSetSnapshot {
            volume_paths: vec![A.to_owned(), "invalid-path".to_owned()],
            status: InventoryStatus::Confirmed,
        };
        let diff = diff_volume_sets(&previous, &current);
        assert_eq!(diff.status, VolumeChangeStatus::CannotCompareUnconfirmedSnapshot);
        assert!(diff.removed.is_empty());
        assert!(diff.added.is_empty());
    }

    #[test]
    fn failed_enumeration_never_looks_like_volume_removal() {
        let old = VolumeSetSnapshot::confirmed([A, B]);
        let failed = VolumeSetSnapshot::unknown(InventoryStatus::EnumerationFailed);
        let d = diff_volume_sets(&old, &failed);
        assert_eq!(d.status, VolumeChangeStatus::CannotCompareUnconfirmedSnapshot);
        assert!(d.removed.is_empty());
        assert!(!d.cache_invalidation_required());
    }
}
