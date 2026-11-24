//! Typed events emitted by BlockHost core for UI shells (Tauri, CLI, service monitors).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreEvent {
    StatusChanged(Box<StatusSnapshot>),
    LogLine {
        index: usize,
        line: String,
    },
    SetupProgress {
        state: String,
        message: String,
        pct: u64,
    },
    Alert {
        level: AlertLevel,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlertLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSummary {
    pub ram_mb: u64,
    pub cores: usize,
    pub tier: String,
    pub recommended_heap_mb: u64,
    pub recommended_max_players: u32,
    pub recommended_view_distance: u8,
    pub recommended_simulation_distance: u8,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupSummary {
    pub state: String,
    pub message: String,
    pub pct: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelSummary {
    pub claim_url: Option<String>,
    pub address: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub app_version: String,
    pub data_dir: String,
    pub hardware: HardwareSummary,
    pub servers: Vec<String>,
    pub current: Option<String>,
    pub port: Option<u16>,
    pub mc_version: Option<String>,
    pub paper_build: Option<u32>,
    pub running: Option<String>,
    pub stopping: bool,
    pub setup: SetupSummary,
    pub tunnel: TunnelSummary,
    pub backups: Vec<String>,
}
