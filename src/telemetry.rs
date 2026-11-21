//! Host CPU and physical-memory telemetry for Alosh Engineering.
//!
//! Sampling is explicit: the host controls cadence and owns the sampler. A
//! failed or first CPU sample is unavailable, never interpreted as idle CPU.

use crate::io_scheduler::{IoTelemetry, TelemetryFreshness};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuCounters {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryCounters {
    pub total_physical: u64,
    pub available_physical: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryStatus {
    Confirmed,
    FirstSample,
    CpuCounterReset,
    CpuUnavailable,
    MemoryUnavailable,
    PlatformUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostTelemetrySnapshot {
    pub cpu_percent: Option<u8>,
    pub memory_pressure: Option<bool>,
    pub memory_used_percent: Option<u8>,
    pub status: TelemetryStatus,
}

impl HostTelemetrySnapshot {
    /// Convert a measured snapshot to scheduler input. Unconfirmed data is
    /// marked unavailable so it cannot trigger optimistic scale-up.
    pub fn to_io_telemetry(self, queue_depth: usize, interactive_foreground: bool) -> IoTelemetry {
        match (self.status, self.cpu_percent, self.memory_pressure) {
            (TelemetryStatus::Confirmed, Some(cpu), Some(memory_pressure)) => IoTelemetry {
                cpu_percent: cpu,
                memory_pressure,
                queue_depth,
                interactive_foreground,
                freshness: TelemetryFreshness::Fresh,
            },
            _ => IoTelemetry {
                cpu_percent: self.cpu_percent.unwrap_or(100),
                memory_pressure: self.memory_pressure.unwrap_or(true),
                queue_depth,
                interactive_foreground,
                freshness: TelemetryFreshness::Unavailable,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsTelemetrySampler {
    previous_cpu: Option<CpuCounters>,
    memory_pressure_threshold_percent: u8,
}

impl Default for WindowsTelemetrySampler {
    fn default() -> Self { Self::new(85) }
}

impl WindowsTelemetrySampler {
    pub const fn new(memory_pressure_threshold_percent: u8) -> Self {
        Self {
            previous_cpu: None,
            memory_pressure_threshold_percent: if memory_pressure_threshold_percent > 100 { 85 } else { memory_pressure_threshold_percent },
        }
    }

    pub fn sample_from_counters(
        &mut self,
        cpu: Option<CpuCounters>,
        memory: Option<MemoryCounters>,
    ) -> HostTelemetrySnapshot {
        let cpu_result = match cpu {
            None => Err(TelemetryStatus::CpuUnavailable),
            Some(current) => {
                let previous = self.previous_cpu.replace(current);
                match previous {
                    None => Err(TelemetryStatus::FirstSample),
                    Some(old) => calculate_cpu_percent(old, current),
                }
            }
        };
        let memory_result = memory.and_then(|m| calculate_memory(m, self.memory_pressure_threshold_percent));
        match (cpu_result, memory_result) {
            (Ok(cpu_percent), Some((used, pressure))) => HostTelemetrySnapshot {
                cpu_percent: Some(cpu_percent), memory_pressure: Some(pressure),
                memory_used_percent: Some(used), status: TelemetryStatus::Confirmed,
            },
            (Err(status), _) => HostTelemetrySnapshot {
                cpu_percent: None, memory_pressure: memory_result.map(|v| v.1),
                memory_used_percent: memory_result.map(|v| v.0), status,
            },
            (Ok(_), None) => HostTelemetrySnapshot {
                cpu_percent: cpu_result.ok(), memory_pressure: None,
                memory_used_percent: None, status: TelemetryStatus::MemoryUnavailable,
            },
        }
    }

    /// Query Windows counters. The first call primes CPU deltas and returns
    /// FirstSample; call again after a host-selected interval for a percentage.
    #[cfg(windows)]
    pub fn sample_windows(&mut self) -> HostTelemetrySnapshot {
        let cpu = windows::read_cpu_counters();
        let memory = windows::read_memory_counters();
        self.sample_from_counters(cpu, memory)
    }

    #[cfg(not(windows))]
    pub fn sample_windows(&mut self) -> HostTelemetrySnapshot {
        HostTelemetrySnapshot {
            cpu_percent: None, memory_pressure: None, memory_used_percent: None,
            status: TelemetryStatus::PlatformUnsupported,
        }
    }
}

fn calculate_cpu_percent(old: CpuCounters, new: CpuCounters) -> Result<u8, TelemetryStatus> {
    let idle = new.idle.checked_sub(old.idle).ok_or(TelemetryStatus::CpuCounterReset)?;
    let kernel = new.kernel.checked_sub(old.kernel).ok_or(TelemetryStatus::CpuCounterReset)?;
    let user = new.user.checked_sub(old.user).ok_or(TelemetryStatus::CpuCounterReset)?;
    let total = kernel.checked_add(user).ok_or(TelemetryStatus::CpuCounterReset)?;
    if total == 0 || idle > total { return Err(TelemetryStatus::CpuCounterReset); }
    let busy = total - idle;
    Ok(((busy as u128 * 100) / total as u128).min(100) as u8)
}

fn calculate_memory(m: MemoryCounters, threshold: u8) -> Option<(u8, bool)> {
    if m.total_physical == 0 || m.available_physical > m.total_physical { return None; }
    let used = m.total_physical - m.available_physical;
    let percent = ((used as u128 * 100) / m.total_physical as u128).min(100) as u8;
    Some((percent, percent >= threshold))
}

#[cfg(windows)]
pub mod windows {
    use super::{CpuCounters, MemoryCounters};
    use std::mem::{size_of, zeroed};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FileTime { low: u32, high: u32 }
    impl FileTime { fn value(self) -> u64 { ((self.high as u64) << 32) | self.low as u64 } }

    #[repr(C)]
    struct MemoryStatusEx {
        length: u32, memory_load: u32,
        total_physical: u64, available_physical: u64,
        total_page_file: u64, available_page_file: u64,
        total_virtual: u64, available_virtual: u64,
        available_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
    }

    pub fn read_cpu_counters() -> Option<CpuCounters> {
        let (mut idle, mut kernel, mut user) = unsafe { (zeroed::<FileTime>(), zeroed::<FileTime>(), zeroed::<FileTime>()) };
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 { return None; }
        Some(CpuCounters { idle: idle.value(), kernel: kernel.value(), user: user.value() })
    }

    pub fn read_memory_counters() -> Option<MemoryCounters> {
        let mut status: MemoryStatusEx = unsafe { zeroed() };
        status.length = size_of::<MemoryStatusEx>() as u32;
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok == 0 { return None; }
        Some(MemoryCounters { total_physical: status.total_physical, available_physical: status.available_physical })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cpu(idle: u64, kernel: u64, user: u64) -> CpuCounters { CpuCounters { idle, kernel, user } }
    fn mem(total: u64, available: u64) -> MemoryCounters { MemoryCounters { total_physical: total, available_physical: available } }

    #[test]
    fn first_sample_never_claims_cpu_idle() {
        let mut s = WindowsTelemetrySampler::default();
        let r = s.sample_from_counters(Some(cpu(10, 20, 10)), Some(mem(100, 50)));
        assert_eq!(r.status, TelemetryStatus::FirstSample);
        assert_eq!(r.cpu_percent, None);
        assert_eq!(r.to_io_telemetry(4, false).freshness, TelemetryFreshness::Unavailable);
    }
    #[test]
    fn calculates_cpu_busy_percentage_from_deltas() {
        let mut s = WindowsTelemetrySampler::default();
        s.sample_from_counters(Some(cpu(10, 20, 10)), Some(mem(100, 50)));
        let r = s.sample_from_counters(Some(cpu(20, 60, 30)), Some(mem(100, 50)));
        assert_eq!(r.cpu_percent, Some(83));
        assert_eq!(r.status, TelemetryStatus::Confirmed);
    }
    #[test]
    fn calculates_memory_pressure_at_threshold() {
        let mut s = WindowsTelemetrySampler::new(85);
        s.sample_from_counters(Some(cpu(0, 0, 0)), Some(mem(100, 10)));
        let r = s.sample_from_counters(Some(cpu(1, 2, 1)), Some(mem(100, 15)));
        assert_eq!(r.memory_used_percent, Some(85));
        assert_eq!(r.memory_pressure, Some(true));
    }
    #[test]
    fn counter_reset_is_not_treated_as_low_cpu() {
        let mut s = WindowsTelemetrySampler::default();
        s.sample_from_counters(Some(cpu(100, 200, 100)), Some(mem(100, 50)));
        let r = s.sample_from_counters(Some(cpu(1, 2, 1)), Some(mem(100, 50)));
        assert_eq!(r.status, TelemetryStatus::CpuCounterReset);
        assert_eq!(r.to_io_telemetry(8, false).freshness, TelemetryFreshness::Unavailable);
    }
    #[test]
    fn invalid_memory_counters_are_rejected() {
        assert_eq!(calculate_memory(mem(0, 0), 85), None);
        assert_eq!(calculate_memory(mem(10, 11), 85), None);
    }
    #[test]
    fn unavailable_cpu_does_not_become_fresh_due_to_memory() {
        let mut s = WindowsTelemetrySampler::default();
        let r = s.sample_from_counters(None, Some(mem(100, 20)));
        assert_eq!(r.status, TelemetryStatus::CpuUnavailable);
        assert_eq!(r.memory_used_percent, Some(80));
        assert_eq!(r.to_io_telemetry(2, false).freshness, TelemetryFreshness::Unavailable);
    }
}
