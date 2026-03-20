use crate::systemd::types::BusType;
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use std::collections::HashMap;
use tokio::sync::mpsc;
use zbus::zvariant::OwnedValue;

use crate::ui::panes::PaneId;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    Terminal(Event),
    Tick,
    Render,
    UnitNew {
        name: String,
        path: String,
        bus_type: BusType,
    },
    UnitRemoved {
        name: String,
        path: String,
        bus_type: BusType,
    },
    PropertiesChanged {
        path: String,
        bus_type: BusType,
        changed: HashMap<String, OwnedValue>,
    },
    LogLine {
        pane_id: PaneId,
        line: String,
    },
    LogStreamEnded {
        pane_id: PaneId,
    },
    CommandResult {
        action: String,
        result: Result<String, String>,
    },
}

pub struct EventHandler {
    tx: mpsc::UnboundedSender<AppEvent>,
    rx: mpsc::UnboundedReceiver<AppEvent>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<AppEvent> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }

    /// Spawn the terminal event reader task.
    pub fn spawn_terminal_reader(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            while let Some(event) = reader.next().await {
                if let Ok(event) = event {
                    if tx.send(AppEvent::Terminal(event)).is_err() {
                        break;
                    }
                }
            }
        });
    }

    /// Spawn the tick timer (~4 Hz for reconciliation).
    pub fn spawn_tick_timer(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
            loop {
                interval.tick().await;
                if tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });
    }

    /// Spawn the render timer (~30 Hz).
    pub fn spawn_render_timer(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(33));
            loop {
                interval.tick().await;
                if tx.send(AppEvent::Render).is_err() {
                    break;
                }
            }
        });
    }
}
