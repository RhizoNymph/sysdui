# Pane Layout

## Scope

**In scope:**
- Layout computation for the binary pane tree (`PaneTree`)
- Equal sizing of same-direction split chains (flattening nested same-direction splits)
- Pixel-perfect rounding with remainder distribution

**Not in scope:**
- Pane splitting/closing logic (uses the tree but does not affect layout algorithm)
- Log rendering within panes
- User interaction for resizing (not currently supported)

## Data/Control Flow

1. `PaneTree::layout(area)` is called with the available screen `Rect`.
2. It delegates to `layout_node(&self.root, area, &mut result)`.
3. For a `Leaf` node: the node's `PaneId` and its assigned `Rect` are pushed to the result.
4. For a `Split` node:
   a. `flatten_same_direction(node, direction)` walks the tree collecting all effective children. Same-direction splits are recursed into; leaves and cross-direction splits stop the walk.
   b. The available dimension (width for Horizontal, height for Vertical) is divided equally among `n` children using integer division. The remainder (`dim % n`) extra pixels are distributed one each to the first `remainder` children.
   c. Each flattened child is recursively laid out with `layout_node()`.

### Example: 3 horizontal panes, 120px wide

```
Tree:  Split(H) -> [Leaf(1), Split(H) -> [Leaf(2), Leaf(3)]]

flatten_same_direction produces: [Leaf(1), Leaf(2), Leaf(3)]

120 / 3 = 40px each, remainder 0
Result: Pane1 @ x=0 w=40, Pane2 @ x=40 w=40, Pane3 @ x=80 w=40
```

### Example: Rounding with 100px, 3 panes

```
100 / 3 = 33 base, remainder 1
Result: Pane1 @ w=34, Pane2 @ w=33, Pane3 @ w=33  (total=100)
```

## Files

| File | Role | Key Exports |
|------|------|-------------|
| `src/ui/panes.rs` | Pane tree data structures and layout algorithm | `PaneTree`, `PaneNode`, `PaneLeaf`, `PaneSplit`, `PaneId`, `SplitDirection`, `layout_node()`, `flatten_same_direction()` |
| `src/ui/mod.rs` | UI rendering; calls `PaneTree::layout()` | `render()`, `LayoutCache` |
| `src/ui/logs.rs` | Renders individual log pane content | `render_log_pane()` |

## Invariants and Constraints

1. **Total dimension conservation**: The sum of all child widths (or heights) must exactly equal the parent's dimension. The remainder distribution algorithm guarantees this.
2. **Contiguous placement**: Children are placed adjacently with no gaps. Each child's position is the cumulative sum of preceding children's sizes.
3. **Cross-direction stops flattening**: A vertical split nested inside a horizontal chain is treated as an opaque child, not flattened. This preserves the 2D grid structure.
4. **Single leaf is identity**: A single leaf node receives the full area unchanged.
5. **Minimum 1 pixel**: With very small areas, children may receive 0 width/height. This is inherent to integer division when `area.width < n`. No minimum is currently enforced.
