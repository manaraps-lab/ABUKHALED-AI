use alosh_storage_topology::device_profile::{VolumeMediaKind, VolumeStorageProfile};
use alosh_storage_topology::io_scheduler::{
    AdaptiveIoScheduler, IoDecisionReason, IoSchedulerConfig, IoTelemetry, TelemetryFreshness,
};
use alosh_storage_topology::storage_probe::{ProbeStatus, StorageClassification, StorageKind};

fn profile(kind: StorageKind) -> VolumeStorageProfile {
    VolumeStorageProfile::from_classifications(vec![(0, StorageClassification { kind, status: ProbeStatus::Confirmed })])
}
fn telemetry(cpu: u8, queue: usize) -> IoTelemetry {
    IoTelemetry { cpu_percent: cpu, memory_pressure: false, queue_depth: queue,
        interactive_foreground: false, freshness: TelemetryFreshness::Fresh }
}

#[test]
fn unknown_profile_stays_at_one_worker() {
    let p = VolumeStorageProfile::from_classifications(Vec::new());
    let mut s = AdaptiveIoScheduler::new(8);
    let d = s.decide(&p, telemetry(10, 100), IoSchedulerConfig::default());
    assert_eq!(d.workers, 1);
}

#[test]
fn rotational_profile_never_exceeds_two_workers() {
    let p = profile(StorageKind::Rotational);
    let mut s = AdaptiveIoScheduler::new(1);
    let d = s.decide(&p, telemetry(10, 100), IoSchedulerConfig::default());
    assert_eq!(d.workers, 2);
    assert_eq!(d.profile_ceiling, 2);
}

#[test]
fn nonrotational_profile_scales_up_gradually() {
    let p = profile(StorageKind::NonRotational);
    let mut s = AdaptiveIoScheduler::new(2);
    let d = s.decide(&p, telemetry(20, 20), IoSchedulerConfig { max_workers: 6, manual_workers: None });
    assert_eq!(d.workers, 3);
    assert_eq!(d.reason, IoDecisionReason::QueueDemand);
}

#[test]
fn cpu_pressure_reduces_workers_by_only_one_step() {
    let p = profile(StorageKind::NonRotational);
    let mut s = AdaptiveIoScheduler::new(5);
    let d = s.decide(&p, telemetry(92, 30), IoSchedulerConfig::default());
    assert_eq!(d.workers, 4);
    assert_eq!(d.reason, IoDecisionReason::CpuPressure);
}

#[test]
fn memory_pressure_and_interactive_work_prioritize_responsiveness() {
    let p = profile(StorageKind::NonRotational);
    let mut s = AdaptiveIoScheduler::new(4);
    let mut t = telemetry(20, 50);
    t.memory_pressure = true;
    assert_eq!(s.decide(&p, t, IoSchedulerConfig::default()).workers, 3);
    t.memory_pressure = false;
    t.interactive_foreground = true;
    let d = s.decide(&p, t, IoSchedulerConfig::default());
    assert_eq!(d.workers, 2);
    assert_eq!(d.reason, IoDecisionReason::InteractivePriority);
}

#[test]
fn stale_telemetry_holds_last_safe_worker_count() {
    let p = profile(StorageKind::NonRotational);
    let mut s = AdaptiveIoScheduler::new(4);
    let mut t = telemetry(99, 0);
    t.freshness = TelemetryFreshness::Stale;
    let d = s.decide(&p, t, IoSchedulerConfig::default());
    assert_eq!(d.workers, 4);
    assert_eq!(d.reason, IoDecisionReason::TelemetryUnavailable);
}

#[test]
fn manual_override_is_bounded_by_profile_and_host_caps() {
    let p = profile(StorageKind::Rotational);
    let mut s = AdaptiveIoScheduler::new(1);
    let d = s.decide(&p, telemetry(5, 50), IoSchedulerConfig { max_workers: 12, manual_workers: Some(10) });
    assert_eq!(d.workers, 2);
    assert_eq!(d.reason, IoDecisionReason::ManualOverride);
}

#[test]
fn zero_worker_configuration_never_stops_progress() {
    let p = profile(StorageKind::NonRotational);
    let mut s = AdaptiveIoScheduler::new(0);
    let d = s.decide(&p, telemetry(95, 0), IoSchedulerConfig { max_workers: 0, manual_workers: None });
    assert_eq!(d.workers, 1);
}

#[test]
fn mixed_profile_is_conservative_and_does_not_scale_above_two() {
    let p = VolumeStorageProfile::from_classifications(vec![
        (0, StorageClassification { kind: StorageKind::NonRotational, status: ProbeStatus::Confirmed }),
        (1, StorageClassification { kind: StorageKind::Rotational, status: ProbeStatus::Confirmed }),
    ]);
    assert_eq!(p.kind, VolumeMediaKind::Mixed);
    let mut s = AdaptiveIoScheduler::new(1);
    let d = s.decide(&p, telemetry(10, 100), IoSchedulerConfig::default());
    assert_eq!(d.workers, 2);
}
