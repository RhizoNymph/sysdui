use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BusType {
    System,
    Session,
}

impl fmt::Display for BusType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::Session => write!(f, "user"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActiveState {
    Active,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Maintenance,
    Reloading,
    Unknown,
}

impl ActiveState {
    pub fn from_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "inactive" => Self::Inactive,
            "failed" => Self::Failed,
            "activating" => Self::Activating,
            "deactivating" => Self::Deactivating,
            "maintenance" => Self::Maintenance,
            "reloading" => Self::Reloading,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Failed => "failed",
            Self::Activating => "activating",
            Self::Deactivating => "deactivating",
            Self::Maintenance => "maintenance",
            Self::Reloading => "reloading",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ActiveState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Loaded,
    NotFound,
    BadSetting,
    Error,
    Masked,
    Unknown,
}

impl LoadState {
    pub fn from_str(s: &str) -> Self {
        match s {
            "loaded" => Self::Loaded,
            "not-found" => Self::NotFound,
            "bad-setting" => Self::BadSetting,
            "error" => Self::Error,
            "masked" => Self::Masked,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UnitFileState {
    Enabled,
    Disabled,
    Static,
    Masked,
    Indirect,
    Generated,
    Transient,
    BadSetting,
    Unknown,
}

#[allow(dead_code)]
impl UnitFileState {
    pub fn from_str(s: &str) -> Self {
        match s {
            "enabled" => Self::Enabled,
            "disabled" => Self::Disabled,
            "static" => Self::Static,
            "masked" => Self::Masked,
            "indirect" => Self::Indirect,
            "generated" => Self::Generated,
            "transient" => Self::Transient,
            "bad-setting" => Self::BadSetting,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Static => "static",
            Self::Masked => "masked",
            Self::Indirect => "indirect",
            Self::Generated => "generated",
            Self::Transient => "transient",
            Self::BadSetting => "bad-setting",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for UnitFileState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Service,
    Timer,
    Socket,
    Mount,
    Target,
    Path,
    Scope,
    Slice,
    Device,
    Automount,
    Swap,
    Snapshot,
    Unknown,
}

impl UnitKind {
    pub fn from_unit_name(name: &str) -> Self {
        if let Some(suffix) = name.rsplit('.').next() {
            match suffix {
                "service" => Self::Service,
                "timer" => Self::Timer,
                "socket" => Self::Socket,
                "mount" => Self::Mount,
                "target" => Self::Target,
                "path" => Self::Path,
                "scope" => Self::Scope,
                "slice" => Self::Slice,
                "device" => Self::Device,
                "automount" => Self::Automount,
                "swap" => Self::Swap,
                "snapshot" => Self::Snapshot,
                _ => Self::Unknown,
            }
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UnitInfo {
    pub name: String,
    pub description: String,
    pub load_state: LoadState,
    pub active_state: ActiveState,
    pub sub_state: String,
    pub unit_kind: UnitKind,
    pub bus_type: BusType,
    pub object_path: String,
    pub unit_file_state: UnitFileState,
}

impl UnitInfo {
    pub fn short_name(&self) -> &str {
        self.name
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(&self.name)
    }

    pub fn is_service(&self) -> bool {
        self.unit_kind == UnitKind::Service
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServiceDetail {
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: u32,
    pub memory_current: u64,
    pub exec_main_start_timestamp: u64,
    pub fragment_path: String,
    pub unit_file_state: String,
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub after: Vec<String>,
    pub description: String,
}

impl ServiceDetail {
    pub fn memory_human(&self) -> String {
        let bytes = self.memory_current;
        if bytes == u64::MAX || bytes == 0 {
            return "N/A".to_string();
        }
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        if bytes >= GB {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.1} KB", bytes as f64 / KB as f64)
        } else {
            format!("{bytes} B")
        }
    }

    pub fn uptime_human(&self) -> String {
        let ts = self.exec_main_start_timestamp;
        if ts == 0 {
            return "N/A".to_string();
        }
        // systemd timestamps are in microseconds since epoch
        let now_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        if now_us <= ts {
            return "just started".to_string();
        }
        let diff_secs = (now_us - ts) / 1_000_000;
        let days = diff_secs / 86400;
        let hours = (diff_secs % 86400) / 3600;
        let mins = (diff_secs % 3600) / 60;
        if days > 0 {
            format!("{days}d {hours:02}h {mins:02}m")
        } else if hours > 0 {
            format!("{hours}h {mins:02}m")
        } else {
            format!("{mins}m")
        }
    }
}
