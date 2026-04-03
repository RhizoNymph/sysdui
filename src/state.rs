use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::journal::filter::Priority;
use crate::ui::panes::{PaneLeaf, PaneNode, PaneSplit, PaneTree, SplitDirection};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionState {
    pub filter_mode: String,
    pub status_filter: String,
    pub list_mode: String,
    pub sort_mode: String,
    pub selected_service: Option<String>,
    pub focused_pane: u64,
    pub next_pane_id: u64,
    pub pane_tree: SerializedPaneNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum SerializedPaneNode {
    Leaf {
        id: u64,
        service_name: String,
        priority: String,
    },
    Split {
        direction: String,
        left: Box<SerializedPaneNode>,
        right: Box<SerializedPaneNode>,
    },
}

impl SerializedPaneNode {
    pub fn from_pane_node(node: &PaneNode) -> Self {
        match node {
            PaneNode::Leaf(leaf) => SerializedPaneNode::Leaf {
                id: leaf.id,
                service_name: leaf.service_name.clone(),
                priority: leaf.priority_filter.as_journalctl_arg().to_string(),
            },
            PaneNode::Split(split) => SerializedPaneNode::Split {
                direction: match split.direction {
                    SplitDirection::Horizontal => "horizontal".to_string(),
                    SplitDirection::Vertical => "vertical".to_string(),
                },
                left: Box::new(SerializedPaneNode::from_pane_node(&split.children[0])),
                right: Box::new(SerializedPaneNode::from_pane_node(&split.children[1])),
            },
        }
    }

    pub fn to_pane_node(&self) -> PaneNode {
        match self {
            SerializedPaneNode::Leaf {
                id,
                service_name,
                priority,
            } => PaneNode::Leaf(PaneLeaf::new(
                *id,
                service_name.clone(),
                Priority::from_str(priority),
            )),
            SerializedPaneNode::Split {
                direction,
                left,
                right,
            } => PaneNode::Split(PaneSplit {
                direction: match direction.as_str() {
                    "horizontal" => SplitDirection::Horizontal,
                    _ => SplitDirection::Vertical,
                },
                children: [
                    Box::new(left.to_pane_node()),
                    Box::new(right.to_pane_node()),
                ],
            }),
        }
    }
}

pub fn state_path() -> PathBuf {
    dirs::state_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/state")
        })
        .join("sysdui")
        .join("session.toml")
}

pub fn save_session(state: &SessionState) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(state)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn load_session() -> Result<Option<SessionState>> {
    let path = state_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let state: SessionState = toml::from_str(&content)?;
    Ok(Some(state))
}

pub fn delete_session() -> Result<()> {
    let path = state_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Save session state to a specific path (used for testing).
#[cfg(test)]
pub fn save_session_to(path: &std::path::Path, state: &SessionState) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(state)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Load session state from a specific path (used for testing).
#[cfg(test)]
pub fn load_session_from(path: &std::path::Path) -> Result<Option<SessionState>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    let state: SessionState = toml::from_str(&content)?;
    Ok(Some(state))
}

impl SessionState {
    /// Create a PaneTree from the serialized state.
    pub fn to_pane_tree(&self) -> PaneTree {
        PaneTree {
            root: self.pane_tree.to_pane_node(),
            next_id: self.next_pane_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_leaf_state() -> SessionState {
        SessionState {
            filter_mode: "user".to_string(),
            status_filter: "active".to_string(),
            list_mode: "all".to_string(),
            sort_mode: "name".to_string(),
            selected_service: Some("nginx.service".to_string()),
            focused_pane: 1,
            next_pane_id: 2,
            pane_tree: SerializedPaneNode::Leaf {
                id: 1,
                service_name: "nginx.service".to_string(),
                priority: "info".to_string(),
            },
        }
    }

    fn sample_split_state() -> SessionState {
        SessionState {
            filter_mode: "both".to_string(),
            status_filter: "all".to_string(),
            list_mode: "include".to_string(),
            sort_mode: "status".to_string(),
            selected_service: None,
            focused_pane: 2,
            next_pane_id: 4,
            pane_tree: SerializedPaneNode::Split {
                direction: "horizontal".to_string(),
                left: Box::new(SerializedPaneNode::Leaf {
                    id: 1,
                    service_name: "nginx.service".to_string(),
                    priority: "info".to_string(),
                }),
                right: Box::new(SerializedPaneNode::Split {
                    direction: "vertical".to_string(),
                    left: Box::new(SerializedPaneNode::Leaf {
                        id: 2,
                        service_name: "sshd.service".to_string(),
                        priority: "warning".to_string(),
                    }),
                    right: Box::new(SerializedPaneNode::Leaf {
                        id: 3,
                        service_name: "docker.service".to_string(),
                        priority: "err".to_string(),
                    }),
                }),
            },
        }
    }

    #[test]
    fn round_trip_serialization() {
        let state = sample_leaf_state();
        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: SessionState = toml::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn leaf_node_serialization() {
        let leaf = SerializedPaneNode::Leaf {
            id: 5,
            service_name: "test.service".to_string(),
            priority: "debug".to_string(),
        };
        let serialized = toml::to_string_pretty(&leaf).unwrap();
        let deserialized: SerializedPaneNode = toml::from_str(&serialized).unwrap();
        assert_eq!(leaf, deserialized);
    }

    #[test]
    fn multi_level_split_serialization() {
        let state = sample_split_state();
        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: SessionState = toml::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn leaf_to_pane_node_round_trip() {
        let leaf = PaneLeaf::new(1, "nginx.service".to_string(), Priority::Info);
        let node = PaneNode::Leaf(leaf);
        let serialized = SerializedPaneNode::from_pane_node(&node);
        let restored = serialized.to_pane_node();

        match restored {
            PaneNode::Leaf(leaf) => {
                assert_eq!(leaf.id, 1);
                assert_eq!(leaf.service_name, "nginx.service");
                assert_eq!(leaf.priority_filter, Priority::Info);
                assert!(leaf.log_buffer.is_empty());
                assert_eq!(leaf.scroll_offset, 0);
                assert!(leaf.search_query.is_empty());
                assert!(leaf.journal_handle.is_none());
            }
            PaneNode::Split(_) => panic!("Expected leaf node"),
        }
    }

    #[test]
    fn split_to_pane_node_round_trip() {
        let state = sample_split_state();
        let tree = state.to_pane_tree();

        assert_eq!(tree.next_id, 4);
        let ids = tree.leaf_ids();
        assert_eq!(ids, vec![1, 2, 3]);

        // Verify leaf contents
        let leaf1 = tree.get_leaf(1).unwrap();
        assert_eq!(leaf1.service_name, "nginx.service");
        assert_eq!(leaf1.priority_filter, Priority::Info);

        let leaf2 = tree.get_leaf(2).unwrap();
        assert_eq!(leaf2.service_name, "sshd.service");
        assert_eq!(leaf2.priority_filter, Priority::Warning);

        let leaf3 = tree.get_leaf(3).unwrap();
        assert_eq!(leaf3.service_name, "docker.service");
        assert_eq!(leaf3.priority_filter, Priority::Err);
    }

    #[test]
    fn save_and_load_round_trip() {
        let state = sample_split_state();
        let dir = std::env::temp_dir().join("sysdui-test-save-load");
        let path = dir.join("session.toml");

        // Clean up from any previous run
        let _ = std::fs::remove_dir_all(&dir);

        save_session_to(&path, &state).unwrap();
        let loaded = load_session_from(&path).unwrap();
        assert_eq!(loaded, Some(state));

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let path = std::env::temp_dir().join("sysdui-test-nonexistent/session.toml");
        let loaded = load_session_from(&path).unwrap();
        assert_eq!(loaded, None);
    }

    #[test]
    fn delete_session_removes_file() {
        let dir = std::env::temp_dir().join("sysdui-test-delete");
        let path = dir.join("session.toml");

        // Clean up from any previous run
        let _ = std::fs::remove_dir_all(&dir);

        let state = sample_leaf_state();
        save_session_to(&path, &state).unwrap();
        assert!(path.exists());

        std::fs::remove_file(&path).unwrap();
        assert!(!path.exists());

        // Deleting again should not error (file already gone)
        let result = load_session_from(&path).unwrap();
        assert_eq!(result, None);

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn priority_round_trips_through_serialization() {
        let priorities = vec![
            (Priority::Err, "err"),
            (Priority::Warning, "warning"),
            (Priority::Notice, "notice"),
            (Priority::Info, "info"),
            (Priority::Debug, "debug"),
        ];

        for (priority, expected_str) in priorities {
            let leaf = PaneLeaf::new(1, "test.service".to_string(), priority);
            let node = PaneNode::Leaf(leaf);
            let serialized = SerializedPaneNode::from_pane_node(&node);

            match &serialized {
                SerializedPaneNode::Leaf {
                    priority: p_str, ..
                } => {
                    assert_eq!(p_str, expected_str);
                }
                _ => panic!("Expected leaf"),
            }

            let restored = serialized.to_pane_node();
            match restored {
                PaneNode::Leaf(leaf) => {
                    assert_eq!(leaf.priority_filter, priority);
                }
                _ => panic!("Expected leaf"),
            }
        }
    }
}
