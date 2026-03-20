use crate::journal::filter::Priority;
use ratatui::prelude::Rect;
use std::collections::VecDeque;

pub const MAX_LOG_LINES: usize = 10_000;

pub type PaneId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug)]
pub enum PaneNode {
    Leaf(PaneLeaf),
    Split(PaneSplit),
}

#[derive(Debug)]
pub struct PaneLeaf {
    pub id: PaneId,
    pub service_name: String,
    pub log_buffer: VecDeque<String>,
    pub scroll_offset: usize, // 0 = tail mode (follow)
    pub search_query: String,
    pub priority_filter: Priority,
    pub journal_handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Debug)]
pub struct PaneSplit {
    pub direction: SplitDirection,
    pub ratio: f32,
    pub children: [Box<PaneNode>; 2],
}

impl PaneLeaf {
    pub fn new(id: PaneId, service_name: String, priority: Priority) -> Self {
        Self {
            id,
            service_name,
            log_buffer: VecDeque::with_capacity(MAX_LOG_LINES),
            scroll_offset: 0,
            search_query: String::new(),
            priority_filter: priority,
            journal_handle: None,
        }
    }

    pub fn push_line(&mut self, line: String) {
        if self.log_buffer.len() >= MAX_LOG_LINES {
            self.log_buffer.pop_front();
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }
        self.log_buffer.push_back(line);
    }

    pub fn is_following(&self) -> bool {
        self.scroll_offset == 0
    }
}

fn dummy_leaf() -> PaneLeaf {
    PaneLeaf::new(0, String::new(), Priority::Info)
}

#[derive(Debug)]
pub struct PaneTree {
    pub root: PaneNode,
    pub next_id: PaneId,
}

impl PaneTree {
    pub fn new(service_name: String, priority: Priority) -> Self {
        Self {
            root: PaneNode::Leaf(PaneLeaf::new(1, service_name, priority)),
            next_id: 2,
        }
    }

    pub fn split(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        service_name: String,
        priority: Priority,
    ) -> Option<PaneId> {
        let new_id = self.next_id;
        let new_leaf = PaneLeaf::new(new_id, service_name, priority);

        if find_and_split(&mut self.root, target_id, direction, new_leaf) {
            self.next_id += 1;
            Some(new_id)
        } else {
            None
        }
    }

    pub fn close(&mut self, target_id: PaneId) -> bool {
        if matches!(&self.root, PaneNode::Leaf(_)) {
            return false;
        }
        close_node(&mut self.root, target_id)
    }

    pub fn leaf_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        collect_leaf_ids(&self.root, &mut ids);
        ids
    }

    pub fn next_leaf_id(&self, current: PaneId) -> PaneId {
        let ids = self.leaf_ids();
        if ids.is_empty() {
            return current;
        }
        if let Some(pos) = ids.iter().position(|&id| id == current) {
            ids[(pos + 1) % ids.len()]
        } else {
            ids[0]
        }
    }

    pub fn get_leaf_mut(&mut self, target_id: PaneId) -> Option<&mut PaneLeaf> {
        find_leaf_mut(&mut self.root, target_id)
    }

    pub fn get_leaf(&self, target_id: PaneId) -> Option<&PaneLeaf> {
        find_leaf(&self.root, target_id)
    }

    pub fn layout(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut result = Vec::new();
        layout_node(&self.root, area, &mut result);
        result
    }
}

fn find_and_split(
    node: &mut PaneNode,
    target_id: PaneId,
    direction: SplitDirection,
    new_leaf: PaneLeaf,
) -> bool {
    if !contains_leaf(node, target_id) {
        return false;
    }
    match node {
        PaneNode::Leaf(leaf) if leaf.id == target_id => {
            let old_node =
                std::mem::replace(node, PaneNode::Leaf(dummy_leaf()));
            *node = PaneNode::Split(PaneSplit {
                direction,
                ratio: 0.5,
                children: [
                    Box::new(old_node),
                    Box::new(PaneNode::Leaf(new_leaf)),
                ],
            });
            true
        }
        PaneNode::Split(split) => {
            // Only recurse into the child that contains the target
            if contains_leaf(&split.children[0], target_id) {
                find_and_split(&mut split.children[0], target_id, direction, new_leaf)
            } else {
                find_and_split(&mut split.children[1], target_id, direction, new_leaf)
            }
        }
        _ => false,
    }
}

fn contains_leaf(node: &PaneNode, target_id: PaneId) -> bool {
    match node {
        PaneNode::Leaf(leaf) => leaf.id == target_id,
        PaneNode::Split(split) => {
            contains_leaf(&split.children[0], target_id)
                || contains_leaf(&split.children[1], target_id)
        }
    }
}

fn close_node(node: &mut PaneNode, target_id: PaneId) -> bool {
    let PaneNode::Split(split) = node else {
        return false;
    };

    let left_is_target =
        matches!(&*split.children[0], PaneNode::Leaf(l) if l.id == target_id);
    let right_is_target =
        matches!(&*split.children[1], PaneNode::Leaf(l) if l.id == target_id);

    if left_is_target {
        if let PaneNode::Leaf(leaf) = &*split.children[0] {
            if let Some(h) = &leaf.journal_handle {
                h.abort();
            }
        }
        let survivor = std::mem::replace(
            &mut split.children[1],
            Box::new(PaneNode::Leaf(dummy_leaf())),
        );
        *node = *survivor;
        true
    } else if right_is_target {
        if let PaneNode::Leaf(leaf) = &*split.children[1] {
            if let Some(h) = &leaf.journal_handle {
                h.abort();
            }
        }
        let survivor = std::mem::replace(
            &mut split.children[0],
            Box::new(PaneNode::Leaf(dummy_leaf())),
        );
        *node = *survivor;
        true
    } else {
        // Recurse into children
        if close_node(&mut split.children[0], target_id) {
            return true;
        }
        close_node(&mut split.children[1], target_id)
    }
}

fn collect_leaf_ids(node: &PaneNode, ids: &mut Vec<PaneId>) {
    match node {
        PaneNode::Leaf(leaf) => ids.push(leaf.id),
        PaneNode::Split(split) => {
            collect_leaf_ids(&split.children[0], ids);
            collect_leaf_ids(&split.children[1], ids);
        }
    }
}

fn find_leaf_mut(node: &mut PaneNode, target_id: PaneId) -> Option<&mut PaneLeaf> {
    match node {
        PaneNode::Leaf(leaf) if leaf.id == target_id => Some(leaf),
        PaneNode::Split(split) => {
            // Check which side contains the target to avoid borrow issues
            if contains_leaf(&split.children[0], target_id) {
                find_leaf_mut(&mut split.children[0], target_id)
            } else {
                find_leaf_mut(&mut split.children[1], target_id)
            }
        }
        _ => None,
    }
}

fn find_leaf(node: &PaneNode, target_id: PaneId) -> Option<&PaneLeaf> {
    match node {
        PaneNode::Leaf(leaf) if leaf.id == target_id => Some(leaf),
        PaneNode::Split(split) => find_leaf(&split.children[0], target_id)
            .or_else(|| find_leaf(&split.children[1], target_id)),
        _ => None,
    }
}

fn layout_node(node: &PaneNode, area: Rect, result: &mut Vec<(PaneId, Rect)>) {
    match node {
        PaneNode::Leaf(leaf) => {
            result.push((leaf.id, area));
        }
        PaneNode::Split(split) => {
            let (a, b) = match split.direction {
                SplitDirection::Horizontal => {
                    let left_width = (area.width as f32 * split.ratio) as u16;
                    let right_width = area.width.saturating_sub(left_width);
                    (
                        Rect::new(area.x, area.y, left_width, area.height),
                        Rect::new(area.x + left_width, area.y, right_width, area.height),
                    )
                }
                SplitDirection::Vertical => {
                    let top_height = (area.height as f32 * split.ratio) as u16;
                    let bottom_height = area.height.saturating_sub(top_height);
                    (
                        Rect::new(area.x, area.y, area.width, top_height),
                        Rect::new(area.x, area.y + top_height, area.width, bottom_height),
                    )
                }
            };
            layout_node(&split.children[0], a, result);
            layout_node(&split.children[1], b, result);
        }
    }
}
