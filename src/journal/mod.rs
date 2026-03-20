pub mod filter;

use anyhow::Result;
use filter::Priority;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::warn;

use crate::event::AppEvent;
use crate::systemd::types::BusType;
use crate::ui::panes::PaneId;

/// Spawn a journalctl tail process for a given unit.
/// Returns a JoinHandle and an abort handle.
pub fn spawn_journal_stream(
    unit_name: &str,
    bus_type: BusType,
    priority: Priority,
    pane_id: PaneId,
    tx: mpsc::UnboundedSender<AppEvent>,
) -> Result<tokio::task::JoinHandle<()>> {
    let mut cmd = Command::new("journalctl");
    cmd.arg("-f")
        .arg("-u")
        .arg(unit_name)
        .arg("-o")
        .arg("short-iso")
        .arg("--no-pager")
        .arg(format!("--priority={}", priority.as_journalctl_arg()));

    if bus_type == BusType::Session {
        cmd.arg("--user");
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture journalctl stdout"))?;

    let handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if tx.send(AppEvent::LogLine { pane_id, line }).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = tx.send(AppEvent::LogStreamEnded { pane_id });
                    break;
                }
                Err(e) => {
                    warn!("Error reading journal line: {e}");
                    let _ = tx.send(AppEvent::LogStreamEnded { pane_id });
                    break;
                }
            }
        }

        // Clean up child process
        let _ = child.kill().await;
    });

    Ok(handle)
}
