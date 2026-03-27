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

/// Flatten a chain of same-direction splits into a list of effective children.
///
/// When a split node has the same direction as its parent, its children are
/// "promoted" so that the entire chain is treated as a single N-way split.
/// Children that are leaves or cross-direction splits stop the recursion.
fn flatten_same_direction<'a>(node: &'a PaneNode, direction: SplitDirection) -> Vec<&'a PaneNode> {
    match node {
        PaneNode::Split(split) if split.direction == direction => {
            let mut result = Vec::new();
            result.extend(flatten_same_direction(&split.children[0], direction));
            result.extend(flatten_same_direction(&split.children[1], direction));
            result
        }
        _ => vec![node],
    }
}

fn layout_node(node: &PaneNode, area: Rect, result: &mut Vec<(PaneId, Rect)>) {
    match node {
        PaneNode::Leaf(leaf) => {
            result.push((leaf.id, area));
        }
        PaneNode::Split(split) => {
            let flat_children = flatten_same_direction(node, split.direction);
            let n = flat_children.len() as u16;
            match split.direction {
                SplitDirection::Horizontal => {
                    let base_width = area.width / n;
                    let remainder = (area.width % n) as usize;
                    let mut x = area.x;
                    for (i, child) in flat_children.iter().enumerate() {
                        let w = base_width + if i < remainder { 1 } else { 0 };
                        layout_node(child, Rect::new(x, area.y, w, area.height), result);
                        x += w;
                    }
                }
                SplitDirection::Vertical => {
                    let base_height = area.height / n;
                    let remainder = (area.height % n) as usize;
                    let mut y = area.y;
                    for (i, child) in flat_children.iter().enumerate() {
                        let h = base_height + if i < remainder { 1 } else { 0 };
                        layout_node(child, Rect::new(area.x, y, area.width, h), result);
                        y += h;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a leaf node with a given id.
    fn leaf(id: PaneId) -> PaneNode {
        PaneNode::Leaf(PaneLeaf::new(id, format!("svc-{}", id), Priority::Info))
    }

    /// Helper to build a split node.
    fn split(direction: SplitDirection, left: PaneNode, right: PaneNode) -> PaneNode {
        PaneNode::Split(PaneSplit {
            direction,
            ratio: 0.5,
            children: [Box::new(left), Box::new(right)],
        })
    }

    /// Helper to perform layout and return results sorted by pane id for deterministic assertions.
    fn do_layout(node: &PaneNode, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut result = Vec::new();
        layout_node(node, area, &mut result);
        result.sort_by_key(|(id, _)| *id);
        result
    }

    #[test]
    fn single_pane_gets_full_area() {
        let node = leaf(1);
        let area = Rect::new(0, 0, 120, 40);
        let layout = do_layout(&node, area);

        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0], (1, Rect::new(0, 0, 120, 40)));
    }

    #[test]
    fn two_horizontal_panes_get_equal_halves() {
        // Split(H) -> [Leaf(1), Leaf(2)]
        let node = split(SplitDirection::Horizontal, leaf(1), leaf(2));
        let area = Rect::new(0, 0, 120, 40);
        let layout = do_layout(&node, area);

        assert_eq!(layout.len(), 2);
        assert_eq!(layout[0], (1, Rect::new(0, 0, 60, 40)));
        assert_eq!(layout[1], (2, Rect::new(60, 0, 60, 40)));
    }

    #[test]
    fn three_horizontal_panes_get_equal_thirds() {
        // Split(H)
        //   Leaf(1)
        //   Split(H)
        //     Leaf(2)
        //     Leaf(3)
        let node = split(
            SplitDirection::Horizontal,
            leaf(1),
            split(SplitDirection::Horizontal, leaf(2), leaf(3)),
        );
        let area = Rect::new(0, 0, 120, 40);
        let layout = do_layout(&node, area);

        assert_eq!(layout.len(), 3);
        // 120 / 3 = 40 each, no remainder
        assert_eq!(layout[0], (1, Rect::new(0, 0, 40, 40)));
        assert_eq!(layout[1], (2, Rect::new(40, 0, 40, 40)));
        assert_eq!(layout[2], (3, Rect::new(80, 0, 40, 40)));
    }

    #[test]
    fn four_horizontal_panes_get_equal_quarters() {
        // Split(H)
        //   Leaf(1)
        //   Split(H)
        //     Leaf(2)
        //     Split(H)
        //       Leaf(3)
        //       Leaf(4)
        let node = split(
            SplitDirection::Horizontal,
            leaf(1),
            split(
                SplitDirection::Horizontal,
                leaf(2),
                split(SplitDirection::Horizontal, leaf(3), leaf(4)),
            ),
        );
        let area = Rect::new(0, 0, 120, 40);
        let layout = do_layout(&node, area);

        assert_eq!(layout.len(), 4);
        // 120 / 4 = 30 each
        assert_eq!(layout[0], (1, Rect::new(0, 0, 30, 40)));
        assert_eq!(layout[1], (2, Rect::new(30, 0, 30, 40)));
        assert_eq!(layout[2], (3, Rect::new(60, 0, 30, 40)));
        assert_eq!(layout[3], (4, Rect::new(90, 0, 30, 40)));
    }

    #[test]
    fn mixed_directions_horizontal_then_vertical() {
        // Split(H)
        //   Leaf(1)
        //   Split(V)      <-- cross-direction, stops flattening
        //     Leaf(2)
        //     Leaf(3)
        //
        // The H split has 2 effective children: Leaf(1) and Split(V).
        // Leaf(1) gets left half (60px wide), Split(V) gets right half (60px wide).
        // Within the V split, Leaf(2) and Leaf(3) each get half the height.
        let node = split(
            SplitDirection::Horizontal,
            leaf(1),
            split(SplitDirection::Vertical, leaf(2), leaf(3)),
        );
        let area = Rect::new(0, 0, 120, 40);
        let layout = do_layout(&node, area);

        assert_eq!(layout.len(), 3);
        assert_eq!(layout[0], (1, Rect::new(0, 0, 60, 40)));
        assert_eq!(layout[1], (2, Rect::new(60, 0, 60, 20)));
        assert_eq!(layout[2], (3, Rect::new(60, 20, 60, 20)));
    }

    #[test]
    fn rounding_distributes_extra_pixels_to_first_panes() {
        // 100px wide, 3 horizontal panes -> 34 + 33 + 33 = 100
        let node = split(
            SplitDirection::Horizontal,
            leaf(1),
            split(SplitDirection::Horizontal, leaf(2), leaf(3)),
        );
        let area = Rect::new(0, 0, 100, 30);
        let layout = do_layout(&node, area);

        assert_eq!(layout.len(), 3);
        // 100 / 3 = 33 base, remainder 1 -> first pane gets +1
        assert_eq!(layout[0], (1, Rect::new(0, 0, 34, 30)));
        assert_eq!(layout[1], (2, Rect::new(34, 0, 33, 30)));
        assert_eq!(layout[2], (3, Rect::new(67, 0, 33, 30)));
    }

    #[test]
    fn three_vertical_panes_get_equal_thirds() {
        let node = split(
            SplitDirection::Vertical,
            leaf(1),
            split(SplitDirection::Vertical, leaf(2), leaf(3)),
        );
        let area = Rect::new(0, 0, 80, 90);
        let layout = do_layout(&node, area);

        assert_eq!(layout.len(), 3);
        // 90 / 3 = 30 each
        assert_eq!(layout[0], (1, Rect::new(0, 0, 80, 30)));
        assert_eq!(layout[1], (2, Rect::new(0, 30, 80, 30)));
        assert_eq!(layout[2], (3, Rect::new(0, 60, 80, 30)));
    }

    #[test]
    fn pane_tree_layout_three_equal() {
        // Test via the PaneTree::layout public API
        let mut tree = PaneTree::new("svc-a".to_string(), Priority::Info);
        // tree has Leaf(1), next_id=2
        tree.split(1, SplitDirection::Horizontal, "svc-b".to_string(), Priority::Info);
        // now: Split(H, Leaf(1), Leaf(2)), next_id=3
        tree.split(2, SplitDirection::Horizontal, "svc-c".to_string(), Priority::Info);
        // now: Split(H, Leaf(1), Split(H, Leaf(2), Leaf(3))), next_id=4

        let area = Rect::new(0, 0, 120, 40);
        let mut layout = tree.layout(area);
        layout.sort_by_key(|(id, _)| *id);

        assert_eq!(layout.len(), 3);
        assert_eq!(layout[0], (1, Rect::new(0, 0, 40, 40)));
        assert_eq!(layout[1], (2, Rect::new(40, 0, 40, 40)));
        assert_eq!(layout[2], (3, Rect::new(80, 0, 40, 40)));
    }
}
