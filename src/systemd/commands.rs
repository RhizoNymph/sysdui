use anyhow::{Context, Result};
use std::process::Command;

use super::types::BusType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
    DaemonReload,
}

impl ServiceAction {
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::DaemonReload => "daemon-reload",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Restart => "Restart",
            Self::Enable => "Enable",
            Self::Disable => "Disable",
            Self::DaemonReload => "Daemon Reload",
        }
    }

    pub fn confirm_key(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::DaemonReload => "daemon-reload",
        }
    }

    pub fn needs_unit(&self) -> bool {
        !matches!(self, Self::DaemonReload)
    }
}

/// Execute a systemctl command. For system services, uses sudo.
/// This is a blocking call — the TUI should be suspended first.
pub fn execute_systemctl(
    action: ServiceAction,
    unit_name: Option<&str>,
    bus_type: BusType,
) -> Result<String> {
    let mut cmd = match bus_type {
        BusType::System => {
            let mut c = Command::new("sudo");
            c.arg("systemctl");
            c
        }
        BusType::Session => {
            let mut c = Command::new("systemctl");
            c.arg("--user");
            c
        }
    };

    cmd.arg(action.verb());

    if let Some(unit) = unit_name
        && action.needs_unit() {
            cmd.arg(unit);
        }

    let output = cmd
        .status()
        .context(format!("Failed to execute systemctl {}", action.verb()))?;

    if output.success() {
        Ok(format!("{} succeeded", action.label()))
    } else {
        anyhow::bail!(
            "{} failed (exit code: {})",
            action.label(),
            output.code().unwrap_or(-1)
        );
    }
}

/// Open a unit file in $EDITOR. For system units, uses sudo $EDITOR.
/// This is a blocking call — the TUI should be suspended first.
pub fn edit_unit_file(fragment_path: &str, bus_type: BusType) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

    let status = match bus_type {
        BusType::System => Command::new("sudo")
            .arg(&editor)
            .arg(fragment_path)
            .status()
            .context("Failed to launch editor with sudo")?,
        BusType::Session => Command::new(&editor)
            .arg(fragment_path)
            .status()
            .context("Failed to launch editor")?,
    };

    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Editor exited with code: {}", status.code().unwrap_or(-1));
    }
}
