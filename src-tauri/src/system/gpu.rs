//! NVIDIA GPU telemetry through NVML (`nvml.dll`, installed with the NVIDIA
//! driver and loaded at runtime). Replaces polling `nvidia-smi`, which spawned
//! a process every few seconds. On PCs without an NVIDIA driver `Nvidia::new`
//! returns an instance that reports nothing; AMD/Intel GPUs keep the usage and
//! VRAM figures Windows itself provides.

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct NvGpu {
    pub name: String,
    pub temperature_c: Option<f32>,
    pub usage: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub core_clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
    pub max_core_clock_mhz: Option<u32>,
    pub power_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub fan_pct: Option<u32>,
}

pub struct Nvidia {
    nvml: Option<Nvml>,
}

impl Nvidia {
    pub fn new() -> Self {
        let nvml = Nvml::init().map_err(|e| tracing::info!("NVML not available ({e}); NVIDIA sensors disabled")).ok();
        Nvidia { nvml }
    }

    #[cfg(test)]
    pub fn available(&self) -> bool {
        self.nvml.is_some()
    }

    pub fn sample(&self) -> Vec<NvGpu> {
        let Some(nvml) = &self.nvml else { return vec![] };
        let count = nvml.device_count().unwrap_or(0);
        (0..count)
            .filter_map(|i| nvml.device_by_index(i).ok())
            .map(|d| {
                let mem = d.memory_info().ok();
                NvGpu {
                    name: d.name().unwrap_or_else(|_| "NVIDIA GPU".into()),
                    temperature_c: d.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f32),
                    usage: d.utilization_rates().ok().map(|u| u.gpu as f32),
                    vram_used: mem.as_ref().map(|m| m.used),
                    vram_total: mem.as_ref().map(|m| m.total),
                    core_clock_mhz: d.clock_info(Clock::Graphics).ok(),
                    mem_clock_mhz: d.clock_info(Clock::Memory).ok(),
                    max_core_clock_mhz: d.max_clock_info(Clock::Graphics).ok(),
                    power_w: d.power_usage().ok().map(|mw| mw as f32 / 1000.0),
                    power_limit_w: d.enforced_power_limit().ok().map(|mw| mw as f32 / 1000.0),
                    fan_pct: d.fan_speed(0).ok(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvml_is_optional_and_sane() {
        let n = Nvidia::new();
        eprintln!("NVML available: {} -> {:?}", n.available(), n.sample());
        for g in n.sample() {
            assert!(!g.name.is_empty());
            if let Some(t) = g.temperature_c {
                assert!((5.0..=120.0).contains(&t), "plausible GPU temperature, got {t}");
            }
            if let (Some(u), Some(t)) = (g.vram_used, g.vram_total) {
                assert!(u <= t);
            }
        }
    }
}
