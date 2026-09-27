//! Phase 4 — monitoring ressources (CPU/RAM/disque) pour éviter qu'une
//! action de l'IA (boucle infinie, build parallèle massif, etc.) sature le
//! VPS. `check_thresholds` est pensé pour être appelé avant/pendant les
//! opérations coûteuses (`exec_command` notamment) et peut refuser une
//! action si le système est déjà sous tension.

use serde::Serialize;
use sysinfo::{Disks, System};

#[derive(Debug, Serialize)]
pub struct ResourceSnapshot {
    pub cpu_usage_percent: f32,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_used_percent: f32,
    pub disks: Vec<DiskUsage>,
}

#[derive(Debug, Serialize)]
pub struct DiskUsage {
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_percent: f32,
}

pub struct ResourceMonitor {
    sys: System,
}

impl ResourceMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self { sys }
    }

    /// tool: get_resource_usage
    /// NB: pour une mesure CPU fiable, sysinfo recommande d'attendre
    /// `System::MINIMUM_CPU_UPDATE_INTERVAL` entre deux refresh — sur un
    /// premier appel juste après `new_all()`, l'usage CPU peut être 0.
    /// Un appel périodique (ex: toutes les 2s dans une tâche de fond) donne
    /// des chiffres bien plus utiles qu'un appel ponctuel à la demande.
    pub fn snapshot(&mut self) -> ResourceSnapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();

        let cpu_usage_percent = self.sys.global_cpu_usage();
        let memory_total_bytes = self.sys.total_memory();
        let memory_used_bytes = self.sys.used_memory();
        let memory_used_percent = if memory_total_bytes > 0 {
            (memory_used_bytes as f32 / memory_total_bytes as f32) * 100.0
        } else {
            0.0
        };

        let disks_list = Disks::new_with_refreshed_list();
        let disks = disks_list
            .iter()
            .map(|d| {
                let total = d.total_space();
                let available = d.available_space();
                let used_percent = if total > 0 {
                    ((total - available) as f32 / total as f32) * 100.0
                } else {
                    0.0
                };
                DiskUsage {
                    mount_point: d.mount_point().to_string_lossy().into_owned(),
                    total_bytes: total,
                    available_bytes: available,
                    used_percent,
                }
            })
            .collect();

        ResourceSnapshot {
            cpu_usage_percent,
            memory_used_bytes,
            memory_total_bytes,
            memory_used_percent,
            disks,
        }
    }

    /// Renvoie une liste d'alertes texte si des seuils sont dépassés.
    /// Seuils codés en dur pour la Phase 4 initiale — à sortir vers
    /// config.toml (section [monitoring]) si tu veux les ajuster sans
    /// recompiler.
    pub fn check_thresholds(&mut self) -> Vec<String> {
        let snap = self.snapshot();
        let mut alerts = Vec::new();

        if snap.cpu_usage_percent > 90.0 {
            alerts.push(format!("CPU à {:.1}% — au-delà du seuil de 90%", snap.cpu_usage_percent));
        }
        if snap.memory_used_percent > 90.0 {
            alerts.push(format!("RAM à {:.1}% — au-delà du seuil de 90%", snap.memory_used_percent));
        }
        for disk in &snap.disks {
            if disk.used_percent > 90.0 {
                alerts.push(format!(
                    "Disque {} à {:.1}% — au-delà du seuil de 90%",
                    disk.mount_point, disk.used_percent
                ));
            }
        }
        alerts
    }
}
