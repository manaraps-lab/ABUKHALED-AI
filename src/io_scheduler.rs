//! Adaptive I/O worker policy. This module schedules work; it never filters,
//! skips, or changes the content/accuracy requirements of that work.

use crate::device_profile::{VolumeMediaKind, VolumeStorageProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryFreshness {
    Fresh,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoTelemetry {
    /// Overall CPU utilization in whole percent, valid range 0..=100.
    pub cpu_percent: u8,
    pub memory_pressure: bool,
    pub queue_depth: usize,
    pub interactive_foreground: bool,
    pub freshness: TelemetryFreshness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoSchedulerConfig {
    /// Hard ceiling imposed by the host's global CPU/I/O budget.
    pub max_workers: usize,
    /// Optional user override. It is still bounded by max_workers and the
    /// confirmed storage-profile ceiling.
    pub manual_workers: Option<usize>,
}

impl Default for IoSchedulerConfig {
    fn default() -> Self { Self { max_workers: 8, manual_workers: None } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoSchedulerDecision {
    pub workers: usize,
    pub profile_ceiling: usize,
    pub reason: IoDecisionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoDecisionReason {
    ManualOverride,
    TelemetryUnavailable,
    InteractivePriority,
    MemoryPressure,
    CpuPressure,
    QueueDemand,
    HoldStable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptiveIoScheduler {
    current_workers: usize,
}

impl AdaptiveIoScheduler {
    pub const fn new(initial_workers: usize) -> Self {
        Self { current_workers: if initial_workers == 0 { 1 } else { initial_workers } }
    }

    pub const fn current_workers(&self) -> usize { self.current_workers }

    pub fn decide(
        &mut self,
        profile: &VolumeStorageProfile,
        telemetry: IoTelemetry,
        config: IoSchedulerConfig,
    ) -> IoSchedulerDecision {
        let host_cap = config.max_workers.max(1);
        let profile_cap = profile.recommended_io_workers(host_cap).max(1).min(host_cap);
        let ceiling = profile_cap.min(host_cap);
        let current = self.current_workers.clamp(1, ceiling);

        if let Some(manual) = config.manual_workers {
            let workers = manual.max(1).min(ceiling);
            self.current_workers = workers;
            return IoSchedulerDecision { workers, profile_ceiling: ceiling, reason: IoDecisionReason::ManualOverride };
        }

        if telemetry.freshness != TelemetryFreshness::Fresh {
            self.current_workers = current;
            return IoSchedulerDecision { workers: current, profile_ceiling: ceiling, reason: IoDecisionReason::TelemetryUnavailable };
        }

        let reason_and_workers = if telemetry.interactive_foreground {
            (IoDecisionReason::InteractivePriority, current.saturating_sub(1).max(1))
        } else if telemetry.memory_pressure {
            (IoDecisionReason::MemoryPressure, current.saturating_sub(1).max(1))
        } else if telemetry.cpu_percent >= 85 {
            (IoDecisionReason::CpuPressure, current.saturating_sub(1).max(1))
        } else if telemetry.cpu_percent <= 60 && telemetry.queue_depth > current && current < ceiling {
            (IoDecisionReason::QueueDemand, current + 1)
        } else {
            (IoDecisionReason::HoldStable, current)
        };
        self.current_workers = reason_and_workers.1.clamp(1, ceiling);
        IoSchedulerDecision { workers: self.current_workers, profile_ceiling: ceiling, reason: reason_and_workers.0 }
    }
}

/// Explain the effective profile cap without guessing a device class.
pub const fn profile_kind(profile: &VolumeStorageProfile) -> VolumeMediaKind { profile.kind }
