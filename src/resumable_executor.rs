//! Resumable, topology-gated I/O task state machine for Alosh Engineering.
//!
//! This module does not perform filesystem reads itself. It coordinates workers
//! and exposes fenced claims/checkpoints so the host's reader/hasher can perform
//! I/O without losing task ownership or confusing inaccessible files with
//! deleted/completed files. Persist snapshots transactionally in the host DB.

use std::collections::BTreeMap;

use crate::io_scheduler::IoSchedulerDecision;
use crate::topology_cache::TopologyCache;
use crate::volume_inventory::normalize_volume_guid_path;
use crate::validate_filesystem_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    pub size_bytes: u64,
    pub modified_time_ns: i128,
    /// Stable filesystem file ID when the platform can provide one.
    pub file_id: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoTaskSpec {
    pub task_id: String,
    pub path: String,
    pub volume_guid: String,
    pub source_identity: SourceIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Leased,
    Retryable,
    Deferred,
    NeedsRevalidation,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFailure {
    AccessDenied,
    SharingViolation,
    DeviceUnavailable,
    SourceChanged,
    ReadError,
    ProcessorError,
    InvalidCheckpoint,
    LeaseExpired,
    ExplicitFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCheckpoint {
    /// Byte offset acknowledged by the processor. It is not proof of EOF.
    pub bytes_processed: u64,
    /// Opaque processor state (for example, a versioned digest state).
    /// The host must persist this in the same DB transaction as bytes_processed.
    pub processor_state: Vec<u8>,
    pub source_identity: SourceIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub spec: IoTaskSpec,
    pub state: TaskState,
    pub checkpoint: Option<TaskCheckpoint>,
    pub attempt: u32,
    pub lease_token: Option<u64>,
    pub lease_deadline_ms: Option<u64>,
    pub lease_topology_generation: Option<u64>,
    pub last_failure: Option<TaskFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskClaim {
    pub task_id: String,
    pub path: String,
    pub volume_guid: String,
    pub token: u64,
    pub topology_generation: u64,
    pub attempt: u32,
    pub resume_from_bytes: u64,
    pub processor_state: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTaskStatus { Added, DuplicateId, InvalidId, InvalidPath, InvalidVolumeGuid }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimStatus { Claimed, NoEligibleTask, WorkerLimitReached, InvalidLeaseDuration, TokenExhausted }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimResult {
    pub status: ClaimStatus,
    pub claim: Option<TaskClaim>,
    /// Number of pending tasks withheld because no confirmed topology was cached.
    pub topology_blocked: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationStatus {
    Applied,
    UnknownTask,
    StaleClaim,
    StaleTopology,
    LeaseExpired,
    InvalidState,
    SourceChanged,
    InvalidCheckpoint,
    NotEndOfFile,
    TokenExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorSnapshot {
    pub records: Vec<TaskRecord>,
    /// Persist this monotonic sequence even when no lease is currently active.
    pub next_token: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumableIoExecutor {
    records: BTreeMap<String, TaskRecord>,
    worker_limit: usize,
    next_token: u64,
}

impl Default for ResumableIoExecutor {
    fn default() -> Self { Self::new(1) }
}

impl ResumableIoExecutor {
    pub fn new(worker_limit: usize) -> Self {
        Self { records: BTreeMap::new(), worker_limit: worker_limit.max(1), next_token: 1 }
    }

    pub fn apply_scheduler_decision(&mut self, decision: &IoSchedulerDecision) {
        self.worker_limit = decision.workers.max(1);
    }

    pub const fn worker_limit(&self) -> usize { self.worker_limit }
    pub fn len(&self) -> usize { self.records.len() }
    pub fn is_empty(&self) -> bool { self.records.is_empty() }
    pub fn get(&self, task_id: &str) -> Option<&TaskRecord> { self.records.get(task_id) }
    pub fn records(&self) -> impl Iterator<Item = &TaskRecord> { self.records.values() }

    pub fn add_task(&mut self, spec: IoTaskSpec) -> AddTaskStatus {
        if spec.task_id.trim().is_empty() || spec.task_id.len() > 512 { return AddTaskStatus::InvalidId; }
        if validate_filesystem_path(&spec.path).is_err() { return AddTaskStatus::InvalidPath; }
        let Some(volume_guid) = normalize_volume_guid_path(&spec.volume_guid) else { return AddTaskStatus::InvalidVolumeGuid; };
        if self.records.contains_key(&spec.task_id) { return AddTaskStatus::DuplicateId; }
        let mut spec = spec;
        spec.volume_guid = volume_guid;
        let id = spec.task_id.clone();
        self.records.insert(id, TaskRecord {
            spec, state: TaskState::Pending, checkpoint: None, attempt: 0,
            lease_token: None, lease_deadline_ms: None, lease_topology_generation: None, last_failure: None,
        });
        AddTaskStatus::Added
    }

    /// Expired claims are made retryable. Their last committed checkpoint is
    /// retained, but only a matching source identity may use it on next claim.
    pub fn reap_expired_leases(&mut self, now_ms: u64) -> usize {
        let mut count = 0;
        for record in self.records.values_mut() {
            if record.state == TaskState::Leased && record.lease_deadline_ms.map_or(false, |d| d <= now_ms) {
                record.state = TaskState::Retryable;
                record.lease_token = None;
                record.lease_deadline_ms = None;
                record.lease_topology_generation = None;
                record.last_failure = Some(TaskFailure::LeaseExpired);
                count += 1;
            }
        }
        count
    }

    /// A task can only be claimed while its volume has a confirmed topology
    /// entry. Unknown topology blocks work; it never marks files missing.
    pub fn claim_next(&mut self, now_ms: u64, lease_ms: u64, cache: &TopologyCache) -> ClaimResult {
        if lease_ms == 0 {
            return ClaimResult { status: ClaimStatus::InvalidLeaseDuration, claim: None, topology_blocked: 0 };
        }
        self.reap_expired_leases(now_ms);
        let active = self.records.values().filter(|r| r.state == TaskState::Leased).count();
        if active >= self.worker_limit {
            return ClaimResult { status: ClaimStatus::WorkerLimitReached, claim: None, topology_blocked: 0 };
        }
        let mut blocked = 0;
        let candidate = self.records.iter().find_map(|(id, r)| {
            if !matches!(r.state, TaskState::Pending | TaskState::Retryable | TaskState::Deferred) { return None; }
            let Some(topology) = cache.get(&r.spec.volume_guid) else { blocked += 1; return None; };
            let active_for_volume = self.records.values().filter(|active| {
                active.state == TaskState::Leased && active.spec.volume_guid == r.spec.volume_guid
            }).count();
            let volume_cap = topology.profile.recommended_io_workers(self.worker_limit).max(1).min(self.worker_limit);
            if active_for_volume >= volume_cap { return None; }
            Some(id.clone())
        });
        let Some(id) = candidate else {
            return ClaimResult { status: ClaimStatus::NoEligibleTask, claim: None, topology_blocked: blocked };
        };
        let topology_generation = cache.generation();
        let Some(next_token) = self.next_token.checked_add(1) else {
            return ClaimResult { status: ClaimStatus::TokenExhausted, claim: None, topology_blocked: blocked };
        };
        let token = self.next_token;
        self.next_token = next_token;
        let Some(record) = self.records.get_mut(&id) else {
            return ClaimResult { status: ClaimStatus::NoEligibleTask, claim: None, topology_blocked: blocked };
        };
        let resume_from_bytes = match &record.checkpoint {
            Some(cp) if cp.source_identity == record.spec.source_identity && cp.bytes_processed <= record.spec.source_identity.size_bytes => cp.bytes_processed,
            _ => {
                record.checkpoint = None;
                0
            }
        };
        let processor_state = record.checkpoint.as_ref().filter(|cp| cp.bytes_processed == resume_from_bytes)
            .map(|cp| cp.processor_state.clone()).unwrap_or_default();
        record.attempt = record.attempt.saturating_add(1);
        record.state = TaskState::Leased;
        record.lease_token = Some(token);
        record.lease_deadline_ms = Some(now_ms.saturating_add(lease_ms));
        record.lease_topology_generation = Some(topology_generation);
        record.last_failure = None;
        ClaimResult {
            status: ClaimStatus::Claimed,
            claim: Some(TaskClaim { task_id: id, path: record.spec.path.clone(), volume_guid: record.spec.volume_guid.clone(), token, topology_generation, attempt: record.attempt, resume_from_bytes, processor_state }),
            topology_blocked: blocked,
        }
    }

    pub fn heartbeat(&mut self, task_id: &str, token: u64, topology_generation: u64, now_ms: u64, lease_ms: u64, cache: &TopologyCache) -> MutationStatus {
        if lease_ms == 0 { return MutationStatus::InvalidState; }
        let Some(record) = self.records.get_mut(task_id) else { return MutationStatus::UnknownTask; };
        if !owns(record, token, now_ms) { return if record.lease_token == Some(token) { MutationStatus::LeaseExpired } else { MutationStatus::StaleClaim }; }
        if !topology_valid(record, topology_generation, cache) { return MutationStatus::StaleTopology; }
        record.lease_deadline_ms = Some(now_ms.saturating_add(lease_ms));
        MutationStatus::Applied
    }

    /// Commit a checkpoint only when the claim still owns the task and the
    /// source identity matches the identity captured when the task was queued.
    pub fn checkpoint(&mut self, task_id: &str, token: u64, topology_generation: u64, now_ms: u64, checkpoint: TaskCheckpoint, cache: &TopologyCache) -> MutationStatus {
        let Some(record) = self.records.get_mut(task_id) else { return MutationStatus::UnknownTask; };
        if !owns(record, token, now_ms) { return if record.lease_token == Some(token) { MutationStatus::LeaseExpired } else { MutationStatus::StaleClaim }; }
        if !topology_valid(record, topology_generation, cache) { return MutationStatus::StaleTopology; }
        if checkpoint.source_identity != record.spec.source_identity { return MutationStatus::SourceChanged; }
        if checkpoint.bytes_processed > record.spec.source_identity.size_bytes { return MutationStatus::InvalidCheckpoint; }
        if record.checkpoint.as_ref().map_or(false, |old| checkpoint.bytes_processed < old.bytes_processed) {
            return MutationStatus::InvalidCheckpoint;
        }
        record.checkpoint = Some(checkpoint);
        MutationStatus::Applied
    }

    /// Completion requires explicit EOF confirmation and a final identity
    /// re-check. A successful read of a prefix is never task completion.
    pub fn complete(&mut self, task_id: &str, token: u64, topology_generation: u64, now_ms: u64, eof_verified: bool, final_identity: &SourceIdentity, cache: &TopologyCache) -> MutationStatus {
        let Some(record) = self.records.get_mut(task_id) else { return MutationStatus::UnknownTask; };
        if !owns(record, token, now_ms) { return if record.lease_token == Some(token) { MutationStatus::LeaseExpired } else { MutationStatus::StaleClaim }; }
        if !topology_valid(record, topology_generation, cache) { return MutationStatus::StaleTopology; }
        if !eof_verified { return MutationStatus::NotEndOfFile; }
        if final_identity != &record.spec.source_identity { return MutationStatus::SourceChanged; }
        if let Some(cp) = &record.checkpoint {
            if cp.source_identity != *final_identity || cp.bytes_processed != final_identity.size_bytes {
                return MutationStatus::InvalidCheckpoint;
            }
        } else if final_identity.size_bytes != 0 {
            // Non-empty files must have an acknowledged full-length checkpoint.
            return MutationStatus::InvalidCheckpoint;
        }
        record.state = TaskState::Completed;
        clear_lease(record);
        record.last_failure = None;
        MutationStatus::Applied
    }

    pub fn defer(&mut self, task_id: &str, token: u64, now_ms: u64) -> MutationStatus {
        self.finish_nonterminal(task_id, token, now_ms, TaskState::Deferred, None)
    }

    pub fn retryable_failure(&mut self, task_id: &str, token: u64, now_ms: u64, failure: TaskFailure) -> MutationStatus {
        if failure != TaskFailure::SourceChanged {
            return self.finish_nonterminal(task_id, token, now_ms, TaskState::Retryable, Some(failure));
        }
        let Some(record) = self.records.get_mut(task_id) else { return MutationStatus::UnknownTask; };
        if !owns(record, token, now_ms) { return if record.lease_token == Some(token) { MutationStatus::LeaseExpired } else { MutationStatus::StaleClaim }; }
        record.state = TaskState::NeedsRevalidation;
        record.checkpoint = None;
        clear_lease(record);
        record.last_failure = Some(TaskFailure::SourceChanged);
        MutationStatus::Applied
    }

    pub fn permanent_failure(&mut self, task_id: &str, token: u64, now_ms: u64, failure: TaskFailure) -> MutationStatus {
        self.finish_nonterminal(task_id, token, now_ms, TaskState::Failed, Some(failure))
    }

    fn finish_nonterminal(&mut self, task_id: &str, token: u64, now_ms: u64, state: TaskState, failure: Option<TaskFailure>) -> MutationStatus {
        let Some(record) = self.records.get_mut(task_id) else { return MutationStatus::UnknownTask; };
        if !owns(record, token, now_ms) { return if record.lease_token == Some(token) { MutationStatus::LeaseExpired } else { MutationStatus::StaleClaim }; }
        record.state = state;
        clear_lease(record);
        record.last_failure = failure;
        MutationStatus::Applied
    }

    /// Explicitly adopt a newly observed source identity after the host has
    /// re-statted the path. Any old checkpoint is discarded; changed content
    /// is never resumed from an offset calculated for the previous file.
    pub fn refresh_source_identity(&mut self, task_id: &str, new_identity: SourceIdentity) -> MutationStatus {
        let Some(record) = self.records.get_mut(task_id) else { return MutationStatus::UnknownTask; };
        if record.state == TaskState::Leased || record.state == TaskState::Completed { return MutationStatus::InvalidState; }
        let needs_revalidation = record.state == TaskState::NeedsRevalidation;
        if record.spec.source_identity != new_identity || needs_revalidation {
            record.spec.source_identity = new_identity;
            record.checkpoint = None;
            record.state = TaskState::Pending;
            record.last_failure = None;
        }
        MutationStatus::Applied
    }

    /// Explicitly retry a terminal failure. This is an operator/host decision,
    /// never an automatic conversion of a failed or inaccessible file to done.
    pub fn requeue_failed(&mut self, task_id: &str) -> MutationStatus {
        let Some(record) = self.records.get_mut(task_id) else { return MutationStatus::UnknownTask; };
        if record.state != TaskState::Failed { return MutationStatus::InvalidState; }
        record.state = TaskState::Retryable;
        record.last_failure = None;
        MutationStatus::Applied
    }

    pub fn snapshot(&self) -> ExecutorSnapshot {
        ExecutorSnapshot { records: self.records.values().cloned().collect(), next_token: self.next_token }
    }

    /// Restore a DB snapshot. In-flight leases are never trusted across process
    /// restarts: they become retryable, and the next token is advanced beyond
    /// all persisted tokens so stale workers are fenced out.
    pub fn restore(snapshot: ExecutorSnapshot, worker_limit: usize) -> Result<Self, RestoreError> {
        let mut records = BTreeMap::new();
        let mut max_token = 0u64;
        let persisted_next_token = snapshot.next_token;
        if persisted_next_token == 0 { return Err(RestoreError::InvalidTokenSequence); }
        for mut record in snapshot.records {
            let Some(normalized_volume) = normalize_volume_guid_path(&record.spec.volume_guid) else {
                return Err(RestoreError::InvalidRecord);
            };
            if record.spec.task_id.trim().is_empty()
                || record.spec.task_id.len() > 512
                || validate_filesystem_path(&record.spec.path).is_err()
            {
                return Err(RestoreError::InvalidRecord);
            }
            record.spec.volume_guid = normalized_volume;
            if records.contains_key(&record.spec.task_id) { return Err(RestoreError::DuplicateTaskId); }

            if record.state == TaskState::Leased {
                let token = record.lease_token.ok_or(RestoreError::InvalidRecord)?;
                if token == 0
                    || record.lease_deadline_ms.is_none()
                    || record.lease_topology_generation.is_none()
                {
                    return Err(RestoreError::InvalidRecord);
                }
                max_token = max_token.max(token);
                // Leases never survive process restart; keep the checkpoint but
                // force a new fenced claim after topology is reconfirmed.
                record.state = TaskState::Retryable;
                record.lease_token = None;
                record.lease_deadline_ms = None;
                record.lease_topology_generation = None;
                record.last_failure = Some(TaskFailure::LeaseExpired);
            } else if record.lease_token.is_some()
                || record.lease_deadline_ms.is_some()
                || record.lease_topology_generation.is_some()
            {
                return Err(RestoreError::InvalidRecord);
            }
            if let Some(cp) = &record.checkpoint {
                if cp.source_identity != record.spec.source_identity || cp.bytes_processed > record.spec.source_identity.size_bytes {
                    record.checkpoint = None;
                    if record.state == TaskState::Completed { return Err(RestoreError::InvalidCompletedRecord); }
                }
            }
            if record.state == TaskState::Completed && record.spec.source_identity.size_bytes > 0
                && record.checkpoint.as_ref().map_or(true, |cp| cp.bytes_processed != record.spec.source_identity.size_bytes) {
                return Err(RestoreError::InvalidCompletedRecord);
            }
            records.insert(record.spec.task_id.clone(), record);
        }
        if persisted_next_token <= max_token { return Err(RestoreError::InvalidTokenSequence); }
        Ok(Self { records, worker_limit: worker_limit.max(1), next_token: persisted_next_token })
    }
}

/// Persistence boundary for the host application's SQLite adapter. Implementors
/// must save the complete snapshot in one database transaction; task state,
/// byte offset, source identity, and opaque processor state must never split.
pub trait ExecutorSnapshotStore {
    type Error;
    fn load_snapshot(&mut self) -> Result<Option<ExecutorSnapshot>, Self::Error>;
    fn save_snapshot_transactional(&mut self, snapshot: &ExecutorSnapshot) -> Result<(), Self::Error>;
}

impl ResumableIoExecutor {
    pub fn persist<S: ExecutorSnapshotStore>(&self, store: &mut S) -> Result<(), S::Error> {
        store.save_snapshot_transactional(&self.snapshot())
    }

    pub fn restore_from_store<S: ExecutorSnapshotStore>(store: &mut S, worker_limit: usize) -> Result<Option<Self>, RestoreStoreError<S::Error>> {
        let Some(snapshot) = store.load_snapshot().map_err(RestoreStoreError::Store)? else { return Ok(None); };
        Self::restore(snapshot, worker_limit).map(Some).map_err(RestoreStoreError::InvalidSnapshot)
    }
}

#[derive(Debug)]
pub enum RestoreStoreError<E> { Store(E), InvalidSnapshot(RestoreError) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError { InvalidRecord, DuplicateTaskId, InvalidCompletedRecord, InvalidTokenSequence, TokenExhausted }

fn owns(record: &TaskRecord, token: u64, now_ms: u64) -> bool {
    record.state == TaskState::Leased && record.lease_token == Some(token) && record.lease_deadline_ms.map_or(false, |deadline| deadline > now_ms)
}
fn topology_valid(record: &TaskRecord, generation: u64, cache: &TopologyCache) -> bool {
    record.lease_topology_generation == Some(generation) && cache.generation() == generation && cache.get(&record.spec.volume_guid).is_some()
}
fn clear_lease(record: &mut TaskRecord) {
    record.lease_token = None;
    record.lease_deadline_ms = None;
    record.lease_topology_generation = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_profile::{PhysicalDeviceProfile, ProfileStatus, VolumeMediaKind, VolumeStorageProfile};
    use crate::storage_probe::{ProbeStatus, StorageClassification, StorageKind};
    use crate::{TopologyStatus, VolumeDiskMapping};

    const VOL: &str = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\";

    fn identity(size: u64) -> SourceIdentity {
        SourceIdentity { size_bytes: size, modified_time_ns: 1234, file_id: Some(vec![1,2,3]) }
    }
    fn spec(id: &str, size: u64) -> IoTaskSpec {
        IoTaskSpec { task_id: id.into(), path: r"C:\data\sample.bin".into(), volume_guid: VOL.into(), source_identity: identity(size) }
    }
    fn cache() -> TopologyCache {
        let mapping = VolumeDiskMapping { disk_numbers: vec![0], status: TopologyStatus::Confirmed };
        let profile = VolumeStorageProfile {
            devices: vec![PhysicalDeviceProfile { disk_number: 0, device_path: r"\\.\PhysicalDrive0".into(), classification: StorageClassification { kind: StorageKind::NonRotational, status: ProbeStatus::Confirmed } }],
            kind: VolumeMediaKind::NonRotational, status: ProfileStatus::Confirmed,
        };
        let mut c = TopologyCache::default();
        assert_eq!(c.insert(VOL, mapping, profile), crate::topology_cache::CacheInsertStatus::Inserted);
        c
    }
    fn claim(e: &mut ResumableIoExecutor) -> TaskClaim {
        e.claim_next(10, 100, &cache()).claim.unwrap()
    }
    fn mixed_cache() -> TopologyCache {
        let mapping = VolumeDiskMapping { disk_numbers: vec![0, 1], status: TopologyStatus::Confirmed };
        let profile = VolumeStorageProfile {
            devices: vec![
                PhysicalDeviceProfile { disk_number: 0, device_path: r"\\.\PhysicalDrive0".into(), classification: StorageClassification { kind: StorageKind::NonRotational, status: ProbeStatus::Confirmed } },
                PhysicalDeviceProfile { disk_number: 1, device_path: r"\\.\PhysicalDrive1".into(), classification: StorageClassification { kind: StorageKind::Rotational, status: ProbeStatus::Confirmed } },
            ],
            kind: VolumeMediaKind::Mixed, status: ProfileStatus::Confirmed,
        };
        let mut c = TopologyCache::default();
        assert_eq!(c.insert(VOL, mapping, profile), crate::topology_cache::CacheInsertStatus::Inserted);
        c
    }

    #[test]
    fn task_is_blocked_until_topology_is_confirmed() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 10));
        let r = e.claim_next(0, 100, &TopologyCache::default());
        assert_eq!(r.status, ClaimStatus::NoEligibleTask); assert_eq!(r.topology_blocked, 1);
        assert_eq!(e.get("a").unwrap().state, TaskState::Pending);
    }
    #[test]
    fn scheduler_decision_limits_concurrency() {
        let mut e = ResumableIoExecutor::new(8); e.apply_scheduler_decision(&IoSchedulerDecision { workers: 1, profile_ceiling: 1, reason: crate::io_scheduler::IoDecisionReason::MemoryPressure });
        e.add_task(spec("a", 1)); e.add_task(spec("b", 1));
        assert_eq!(e.claim_next(0, 100, &cache()).status, ClaimStatus::Claimed);
        assert_eq!(e.claim_next(0, 100, &cache()).status, ClaimStatus::WorkerLimitReached);
    }
    #[test]
    fn mixed_volume_caps_concurrency_at_two_even_when_host_budget_is_higher() {
        let mut e = ResumableIoExecutor::new(8);
        e.add_task(spec("a", 1)); e.add_task(spec("b", 1)); e.add_task(spec("c", 1));
        let c = mixed_cache();
        assert_eq!(e.claim_next(0, 100, &c).status, ClaimStatus::Claimed);
        assert_eq!(e.claim_next(1, 100, &c).status, ClaimStatus::Claimed);
        assert_eq!(e.claim_next(2, 100, &c).status, ClaimStatus::NoEligibleTask);
    }

    #[test]
    fn checkpoint_and_completion_require_full_eof() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        assert_eq!(e.checkpoint("a", c.token, c.topology_generation, 20, TaskCheckpoint { bytes_processed: 4, processor_state: vec![9], source_identity: identity(4) }, &cache()), MutationStatus::Applied);
        assert_eq!(e.complete("a", c.token, c.topology_generation, 20, false, &identity(4), &cache()), MutationStatus::NotEndOfFile);
        assert_eq!(e.complete("a", c.token, c.topology_generation, 20, true, &identity(4), &cache()), MutationStatus::Applied);
        assert_eq!(e.get("a").unwrap().state, TaskState::Completed);
    }
    #[test]
    fn nonempty_file_cannot_complete_without_full_checkpoint() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        assert_eq!(e.complete("a", c.token, c.topology_generation, 20, true, &identity(4), &cache()), MutationStatus::InvalidCheckpoint);
    }
    #[test]
    fn source_change_prevents_checkpoint_and_completion() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        assert_eq!(e.checkpoint("a", c.token, c.topology_generation, 20, TaskCheckpoint { bytes_processed: 2, processor_state: vec![], source_identity: identity(3) }, &cache()), MutationStatus::SourceChanged);
        assert_eq!(e.complete("a", c.token, c.topology_generation, 20, true, &identity(5), &cache()), MutationStatus::SourceChanged);
    }
    #[test]
    fn restore_rejects_stale_lease_fields_on_non_leased_records() {
        let record = TaskRecord {
            spec: IoTaskSpec {
                task_id: "stale-lease".into(),
                path: r"C:\data\file.bin".into(),
                volume_guid: r"\\?\Volume{01234567-89AB-CDEF-0123-456789ABCDEF}".into(),
                source_identity: SourceIdentity { size_bytes: 0, modified_time_ns: 0, file_id: None },
            },
            state: TaskState::Retryable,
            checkpoint: None,
            attempt: 0,
            lease_token: Some(1),
            lease_deadline_ms: None,
            lease_topology_generation: None,
            last_failure: None,
        };
        let snapshot = ExecutorSnapshot { records: vec![record], next_token: 2 };
        assert_eq!(ResumableIoExecutor::restore(snapshot, 1).unwrap_err(), RestoreError::InvalidRecord);
    }

    #[test]
    fn expired_lease_is_reclaimed_and_old_token_is_fenced() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let old = claim(&mut e);
        assert_eq!(e.reap_expired_leases(200), 1);
        let new = e.claim_next(201, 100, &cache()).claim.unwrap();
        assert_ne!(old.token, new.token);
        assert_eq!(e.heartbeat("a", old.token, old.topology_generation, 202, 100, &cache()), MutationStatus::StaleClaim);
    }
    #[test]
    fn expired_claim_cannot_commit_even_before_reaper_runs() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        assert_eq!(e.checkpoint("a", c.token, c.topology_generation, 111, TaskCheckpoint { bytes_processed: 1, processor_state: vec![], source_identity: identity(4) }, &cache()), MutationStatus::LeaseExpired);
    }

    #[test]
    fn checkpoint_survives_retry_and_resumes_at_committed_offset() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 10)); let c = claim(&mut e);
        assert_eq!(e.checkpoint("a", c.token, c.topology_generation, 20, TaskCheckpoint { bytes_processed: 5, processor_state: vec![7,8], source_identity: identity(10) }, &cache()), MutationStatus::Applied);
        assert_eq!(e.retryable_failure("a", c.token, 20, TaskFailure::ReadError), MutationStatus::Applied);
        let next = e.claim_next(20, 100, &cache()).claim.unwrap();
        assert_eq!(next.resume_from_bytes, 5); assert_eq!(next.processor_state, vec![7,8]);
    }
    #[test]
    fn restore_requeues_leased_work_and_fences_old_token() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let old = claim(&mut e);
        let restored = ResumableIoExecutor::restore(e.snapshot(), 1).unwrap();
        let mut restored = restored;
        assert_eq!(restored.get("a").unwrap().state, TaskState::Retryable);
        let new = restored.claim_next(30, 100, &cache()).claim.unwrap();
        assert_ne!(old.token, new.token);
        assert_eq!(restored.heartbeat("a", old.token, old.topology_generation, 31, 100, &cache()), MutationStatus::StaleClaim);
    }
    #[test]
    fn token_sequence_survives_restart_even_when_previous_lease_was_cleared() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let old = claim(&mut e);
        e.permanent_failure("a", old.token, 20, TaskFailure::ProcessorError);
        e.requeue_failed("a");
        let mut restored = ResumableIoExecutor::restore(e.snapshot(), 1).unwrap();
        let new = restored.claim_next(30, 100, &cache()).claim.unwrap();
        assert_ne!(old.token, new.token);
    }

    #[test]
    fn inaccessible_file_is_retryable_not_completed() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        assert_eq!(e.retryable_failure("a", c.token, 20, TaskFailure::AccessDenied), MutationStatus::Applied);
        assert_eq!(e.get("a").unwrap().state, TaskState::Retryable);
    }
    #[test]
    fn explicit_permanent_failure_remains_visible_until_requeued() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        e.permanent_failure("a", c.token, 20, TaskFailure::ProcessorError);
        assert_eq!(e.get("a").unwrap().state, TaskState::Failed);
        assert_eq!(e.requeue_failed("a"), MutationStatus::Applied);
        assert_eq!(e.get("a").unwrap().state, TaskState::Retryable);
    }
    #[test]
    fn invalid_checkpoint_regression_is_rejected() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 10)); let c = claim(&mut e);
        let mk = |n| TaskCheckpoint { bytes_processed: n, processor_state: vec![], source_identity: identity(10) };
        assert_eq!(e.checkpoint("a", c.token, c.topology_generation, 20, mk(7), &cache()), MutationStatus::Applied);
        assert_eq!(e.checkpoint("a", c.token, c.topology_generation, 20, mk(6), &cache()), MutationStatus::InvalidCheckpoint);
    }
    #[test]
    fn source_identity_refresh_discards_old_checkpoint() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 10)); let c = claim(&mut e);
        e.checkpoint("a", c.token, c.topology_generation, 20, TaskCheckpoint { bytes_processed: 5, processor_state: vec![1], source_identity: identity(10) }, &cache());
        e.retryable_failure("a", c.token, 20, TaskFailure::SourceChanged);
        assert_eq!(e.refresh_source_identity("a", identity(12)), MutationStatus::Applied);
        assert_eq!(e.get("a").unwrap().checkpoint, None);
        assert_eq!(e.get("a").unwrap().state, TaskState::Pending);
    }

    #[test]
    fn source_change_blocks_reclaim_until_host_revalidates_identity() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 10)); let c = claim(&mut e);
        assert_eq!(e.retryable_failure("a", c.token, 20, TaskFailure::SourceChanged), MutationStatus::Applied);
        assert_eq!(e.get("a").unwrap().state, TaskState::NeedsRevalidation);
        assert_eq!(e.claim_next(21, 100, &cache()).status, ClaimStatus::NoEligibleTask);
        assert_eq!(e.refresh_source_identity("a", identity(10)), MutationStatus::Applied);
        assert_eq!(e.claim_next(22, 100, &cache()).status, ClaimStatus::Claimed);
    }

    #[test]
    fn invalid_or_duplicate_task_is_rejected() {
        let mut e = ResumableIoExecutor::new(1);
        assert_eq!(e.add_task(spec("", 1)), AddTaskStatus::InvalidId);
        assert_eq!(e.add_task(spec("a", 1)), AddTaskStatus::Added);
        assert_eq!(e.add_task(spec("a", 1)), AddTaskStatus::DuplicateId);
    }
    #[test]
    fn empty_file_can_complete_after_eof_verification() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 0)); let c = claim(&mut e);
        assert_eq!(e.complete("a", c.token, c.topology_generation, 20, true, &identity(0), &cache()), MutationStatus::Applied);
    }
    #[test]
    fn deferred_work_can_be_claimed_later() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        assert_eq!(e.defer("a", c.token, 20), MutationStatus::Applied);
        assert_eq!(e.get("a").unwrap().state, TaskState::Deferred);
        assert_eq!(e.claim_next(20, 100, &cache()).status, ClaimStatus::Claimed);
    }
    #[test]
    fn topology_generation_change_fences_active_worker() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 4)); let c = claim(&mut e);
        let mut changed = cache();
        let diff = crate::volume_inventory::VolumeSetDiff { added: vec![], removed: vec![VOL.trim_end_matches('\\').into()], status: crate::volume_inventory::VolumeChangeStatus::Confirmed };
        changed.invalidate_from_diff(&diff);
        assert_eq!(e.checkpoint("a", c.token, c.topology_generation, 20, TaskCheckpoint { bytes_processed: 2, processor_state: vec![], source_identity: identity(4) }, &changed), MutationStatus::StaleTopology);
    }

    #[test]
    fn heartbeat_extends_only_current_claim() {
        let mut e = ResumableIoExecutor::new(1); e.add_task(spec("a", 1)); let c = claim(&mut e);
        assert_eq!(e.heartbeat("a", c.token, c.topology_generation, 50, 500, &cache()), MutationStatus::Applied);
        assert_eq!(e.reap_expired_leases(200), 0);
    }
}
