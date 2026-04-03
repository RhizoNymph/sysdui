use anyhow::{Context, Result};
use std::collections::HashMap;
use tokio::sync::mpsc;
use zbus::Connection;
use zbus::zvariant::OwnedValue;

use super::types::*;
use crate::event::AppEvent;

type ListUnitsEntry = (
    String,
    String,
    String,
    String,
    String,
    String,
    zbus::zvariant::OwnedObjectPath,
    u32,
    String,
    zbus::zvariant::OwnedObjectPath,
);

/// Connect to the system D-Bus.
pub async fn system_bus() -> Result<Connection> {
    Connection::system()
        .await
        .context("Failed to connect to system D-Bus")
}

/// Connect to the session (user) D-Bus.
pub async fn session_bus() -> Result<Connection> {
    Connection::session()
        .await
        .context("Failed to connect to session D-Bus")
}

/// List all units from a given bus connection.
pub async fn list_units(conn: &Connection, bus_type: BusType) -> Result<Vec<UnitInfo>> {
    let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(conn)
        .destination("org.freedesktop.systemd1")?
        .path("/org/freedesktop/systemd1")?
        .interface("org.freedesktop.systemd1.Manager")?
        .build()
        .await?;

    let reply = proxy
        .call_method("ListUnits", &())
        .await
        .context("Failed to call ListUnits")?;

    let units: Vec<ListUnitsEntry> = reply.body().deserialize()?;

    let result: Vec<UnitInfo> = units
        .into_iter()
        .map(
            |(name, description, load_state, active_state, sub_state, _, object_path, ..)| {
                UnitInfo {
                    unit_kind: UnitKind::from_unit_name(&name),
                    load_state: LoadState::from_str(&load_state),
                    active_state: ActiveState::from_str(&active_state),
                    sub_state,
                    bus_type,
                    object_path: object_path.to_string(),
                    name,
                    description,
                    unit_file_state: UnitFileState::Unknown,
                }
            },
        )
        .collect();

    Ok(result)
}

/// List all unit files from a given bus connection.
/// Returns (unit_file_path, enablement_state) pairs.
pub async fn list_unit_files(conn: &Connection, bus_type: BusType) -> Result<Vec<UnitInfo>> {
    let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(conn)
        .destination("org.freedesktop.systemd1")?
        .path("/org/freedesktop/systemd1")?
        .interface("org.freedesktop.systemd1.Manager")?
        .build()
        .await?;

    let reply = proxy
        .call_method("ListUnitFiles", &())
        .await
        .context("Failed to call ListUnitFiles")?;

    let files: Vec<(String, String)> = reply.body().deserialize()?;

    let result: Vec<UnitInfo> = files
        .into_iter()
        .filter_map(|(path, state)| {
            // Extract filename from path
            let name = path.rsplit('/').next()?.to_string();
            // Only include .service files
            if !name.ends_with(".service") {
                return None;
            }
            Some(UnitInfo {
                unit_kind: UnitKind::Service,
                load_state: LoadState::Unknown,
                active_state: ActiveState::Inactive,
                sub_state: String::new(),
                bus_type,
                object_path: String::new(),
                name,
                description: String::new(),
                unit_file_state: UnitFileState::from_str(&state),
            })
        })
        .collect();

    Ok(result)
}

/// Subscribe to Manager signals so we receive UnitNew/UnitRemoved.
pub async fn subscribe(conn: &Connection) -> Result<()> {
    let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(conn)
        .destination("org.freedesktop.systemd1")?
        .path("/org/freedesktop/systemd1")?
        .interface("org.freedesktop.systemd1.Manager")?
        .build()
        .await?;

    proxy
        .call_method("Subscribe", &())
        .await
        .context("Failed to subscribe to systemd signals")?;

    Ok(())
}

/// Fetch detailed properties for a unit.
pub async fn get_service_detail(conn: &Connection, object_path: &str) -> Result<ServiceDetail> {
    let unit_proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(conn)
        .destination("org.freedesktop.systemd1")?
        .path(object_path)?
        .interface("org.freedesktop.DBus.Properties")?
        .build()
        .await?;

    let mut detail = ServiceDetail::default();

    let unit_iface = "org.freedesktop.systemd1.Unit";
    let svc_iface = "org.freedesktop.systemd1.Service";

    if let Ok(reply) = unit_proxy.call_method("GetAll", &(unit_iface,)).await
        && let Ok(props) = reply.body().deserialize::<HashMap<String, OwnedValue>>() {
            if let Some(v) = props.get("ActiveState") {
                detail.active_state = try_string(v);
            }
            if let Some(v) = props.get("SubState") {
                detail.sub_state = try_string(v);
            }
            if let Some(v) = props.get("FragmentPath") {
                detail.fragment_path = try_string(v);
            }
            if let Some(v) = props.get("UnitFileState") {
                detail.unit_file_state = try_string(v);
            }
            if let Some(v) = props.get("Description") {
                detail.description = try_string(v);
            }
            if let Some(v) = props.get("Requires") {
                detail.requires = try_string_vec(v);
            }
            if let Some(v) = props.get("Wants") {
                detail.wants = try_string_vec(v);
            }
            if let Some(v) = props.get("After") {
                detail.after = try_string_vec(v);
            }
        }

    if let Ok(reply) = unit_proxy.call_method("GetAll", &(svc_iface,)).await
        && let Ok(props) = reply.body().deserialize::<HashMap<String, OwnedValue>>() {
            if let Some(v) = props.get("MainPID") {
                detail.main_pid = try_u32(v);
            }
            if let Some(v) = props.get("MemoryCurrent") {
                detail.memory_current = try_u64(v);
            }
            if let Some(v) = props.get("ExecMainStartTimestamp") {
                detail.exec_main_start_timestamp = try_u64(v);
            }
        }

    Ok(detail)
}

fn try_string(v: &OwnedValue) -> String {
    // zbus 5: downcast_ref returns Result<T, Error> where T is owned
    v.downcast_ref::<String>().ok().unwrap_or_default()
}

fn try_string_vec(v: &OwnedValue) -> Vec<String> {
    use zbus::zvariant;
    if let Ok(zvariant::Value::Array(arr)) = zvariant::Value::try_from(v).as_ref() {
        return arr
            .iter()
            .filter_map(|item| {
                if let zvariant::Value::Str(s) = item {
                    Some(s.to_string())
                } else {
                    None
                }
            })
            .collect();
    }
    vec![]
}

fn try_u32(v: &OwnedValue) -> u32 {
    v.downcast_ref::<u32>().ok().unwrap_or(0)
}

fn try_u64(v: &OwnedValue) -> u64 {
    v.downcast_ref::<u64>().ok().unwrap_or(0)
}

/// Spawn a task that listens for D-Bus signals and forwards them as AppEvents.
pub fn spawn_signal_listener(
    conn: Connection,
    bus_type: BusType,
    tx: mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        use futures::StreamExt;

        // Listen for PropertiesChanged signals
        let props_rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .unwrap()
            .member("PropertiesChanged")
            .unwrap()
            .build();

        let tx2 = tx.clone();
        let conn2 = conn.clone();
        tokio::spawn(async move {
            if let Ok(mut stream) =
                zbus::MessageStream::for_match_rule(props_rule, &conn2, None).await
            {
                while let Some(Ok(msg)) = stream.next().await {
                    let path = msg.header().path().map(|p| p.to_string());
                    if let Some(path) = path {
                        let changed: HashMap<String, OwnedValue> = msg
                            .body()
                            .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                            .map(|(_, props, _)| props)
                            .unwrap_or_default();

                        let _ = tx2.send(AppEvent::PropertiesChanged {
                            path,
                            bus_type,
                            changed,
                        });
                    }
                }
            }
        });

        // Listen for UnitNew and UnitRemoved signals
        let mgr_rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .path("/org/freedesktop/systemd1")
            .unwrap()
            .interface("org.freedesktop.systemd1.Manager")
            .unwrap()
            .build();

        if let Ok(mut stream) = zbus::MessageStream::for_match_rule(mgr_rule, &conn, None).await {
            while let Some(Ok(msg)) = stream.next().await {
                let member = msg.header().member().map(|m| m.to_string());
                match member.as_deref() {
                    Some("UnitNew") => {
                        if let Ok((name, path)) = msg
                            .body()
                            .deserialize::<(String, zbus::zvariant::OwnedObjectPath)>()
                        {
                            let _ = tx.send(AppEvent::UnitNew {
                                name,
                                path: path.to_string(),
                                bus_type,
                            });
                        }
                    }
                    Some("UnitRemoved") => {
                        if let Ok((name, path)) = msg
                            .body()
                            .deserialize::<(String, zbus::zvariant::OwnedObjectPath)>()
                        {
                            let _ = tx.send(AppEvent::UnitRemoved {
                                name,
                                path: path.to_string(),
                                bus_type,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    });
}
