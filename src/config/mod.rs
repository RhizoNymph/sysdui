pub mod keys;

use crate::journal::filter::Priority;
use anyhow::Result;
use keys::{KeyBindings, apply_config_keys};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub confirmations: ConfirmationsConfig,
    pub filter: FilterConfig,
    pub log: LogConfig,
    pub sort: SortConfig,
    pub keys: KeyBindings,
}

#[derive(Debug, Clone)]
pub struct ConfirmationsConfig {
    pub global: bool,
    pub start: bool,
    pub stop: bool,
    pub restart: bool,
    pub enable: bool,
    pub disable: bool,
    pub daemon_reload: bool,
}

#[derive(Debug, Clone)]
pub struct FilterConfig {
    pub mode: String,
    pub show: String,
    pub status: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub priority: Priority,
}

#[derive(Debug, Clone)]
pub struct SortConfig {
    pub default: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            confirmations: ConfirmationsConfig {
                global: true,
                start: true,
                stop: true,
                restart: true,
                enable: true,
                disable: true,
                daemon_reload: true,
            },
            filter: FilterConfig {
                mode: "all".to_string(),
                show: "both".to_string(),
                status: "all".to_string(),
                include: vec![],
                exclude: vec![],
            },
            log: LogConfig {
                priority: Priority::Info,
            },
            sort: SortConfig {
                default: "name".to_string(),
            },
            keys: KeyBindings::default(),
        }
    }
}

impl Config {
    pub fn needs_confirmation(&self, action: &str) -> bool {
        if !self.confirmations.global {
            return false;
        }
        match action {
            "start" => self.confirmations.start,
            "stop" => self.confirmations.stop,
            "restart" => self.confirmations.restart,
            "enable" => self.confirmations.enable,
            "disable" => self.confirmations.disable,
            "daemon-reload" => self.confirmations.daemon_reload,
            _ => true,
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("sysdui")
        .join("config.toml")
}

pub fn load_config() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(&path)?;
    let raw: RawConfig = toml::from_str(&content)?;
    let mut config = Config::default();

    if let Some(c) = raw.confirmations {
        if let Some(v) = c.global {
            config.confirmations.global = v;
        }
        if let Some(v) = c.start {
            config.confirmations.start = v;
        }
        if let Some(v) = c.stop {
            config.confirmations.stop = v;
        }
        if let Some(v) = c.restart {
            config.confirmations.restart = v;
        }
        if let Some(v) = c.enable {
            config.confirmations.enable = v;
        }
        if let Some(v) = c.disable {
            config.confirmations.disable = v;
        }
        if let Some(v) = c.daemon_reload {
            config.confirmations.daemon_reload = v;
        }
    }

    if let Some(f) = raw.filter {
        if let Some(v) = f.mode {
            config.filter.mode = v;
        }
        if let Some(v) = f.show {
            config.filter.show = v;
        }
        if let Some(v) = f.status {
            config.filter.status = v;
        }
        if let Some(v) = f.include {
            config.filter.include = v;
        }
        if let Some(v) = f.exclude {
            config.filter.exclude = v;
        }
    }

    if let Some(l) = raw.log {
        if let Some(v) = l.priority {
            config.log.priority = Priority::from_str(&v);
        }
    }

    if let Some(s) = raw.sort {
        if let Some(v) = s.default {
            config.sort.default = v;
        }
    }

    if let Some(k) = raw.keys {
        apply_config_keys(&mut config.keys, &k);
    }

    Ok(config)
}

// Raw TOML deserialization types (all fields optional for partial configs)
#[derive(Deserialize)]
struct RawConfig {
    confirmations: Option<RawConfirmations>,
    filter: Option<RawFilter>,
    log: Option<RawLog>,
    sort: Option<RawSort>,
    keys: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct RawConfirmations {
    global: Option<bool>,
    start: Option<bool>,
    stop: Option<bool>,
    restart: Option<bool>,
    enable: Option<bool>,
    disable: Option<bool>,
    daemon_reload: Option<bool>,
}

#[derive(Deserialize)]
struct RawFilter {
    mode: Option<String>,
    show: Option<String>,
    status: Option<String>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawLog {
    priority: Option<String>,
}

#[derive(Deserialize)]
struct RawSort {
    default: Option<String>,
}
