# Pane Management

## Scope

### In Scope

- Binary tree data structure (`PaneTree`) for organizing panes in a recursive split layout
- Horizontal and vertical splitting of existing panes
- Closing panes with sibling promotion
- Focus cycling across panes (keyboard and mouse)
- Recursive layout computation that maps the tree to concrete `Rect` regions
- Per-pane state management (`PaneLeaf`): service name, log buffer, scroll offset, search query, priority filter, journal handle
- Log buffer lifecycle: capped ring buffer with automatic eviction, scroll offset adjustment on eviction
- SplitPrompt input mode overlay for choosing split direction
- Context menu integration for split/close actions on both panes and sidebar services
- Mouse interactions: left-click to focus, middle-click to close, right-click for context menu, scroll wheel for log scrolling
- PaneId generation and uniqueness via monotonic counter

### Out of Scope

- Log content rendering and search highlighting (handled by `src/ui/logs.rs` and `src/journal/filter.rs`)
- Service browsing and sidebar (handled by `src/ui/sidebar.rs`)
- Journal stream spawning and log ingestion (handled by `src/journal/` module)
- Service detail panel rendering (handled by `src/ui/detail.rs`)
- Keybinding configuration and remapping (handled by `src/config/keys.rs`)

---

## Data/Control Flow

### Tree Structure

The pane system is built on a recursive binary tree. `PaneTree` holds a `root: PaneNode` and a monotonically increasing `next_id: PaneId` counter. `PaneNode` is an enum with two variants:

- `Leaf(PaneLeaf)` -- a terminal node representing a visible pane with its own state.
- `Split(PaneSplit)` -- an internal node containing a `SplitDirection` (Horizontal or Vertical), a `ratio: f32` (always 0.5 currently), and exactly two boxed `PaneNode` children.

On initialization, `PaneTree::new()` creates a single `Leaf` with `id=1` and sets `next_id=2`. The tree always has at least one leaf; closing the last remaining leaf is a no-op.

### Splitting a Pane

There are three entry paths to splitting:

1. **Keyboard (SplitPrompt)**: User presses `p` (mapped to `KeyAction::PinPane`) while in Normal mode. This transitions `input_mode` to `InputMode::SplitPrompt`. A centered overlay dialog renders asking `[h]orizontal / [v]ertical`. The user presses `h` or `v`, which calls `App::split_pane(direction)`. Any other key cancels and returns to Normal mode.

2. **Right-click context menu on a pane**: Opens a context menu with "Split Horizontal", "Split Vertical", "Close Pane" options. Selecting a split option sets `focused_pane` to the target pane and calls `split_pane()`.

3. **Right-click context menu on a sidebar service**: Opens a context menu with "Split Into Pane (H)" and "Split Into Pane (V)" options. These call `PaneTree::split()` directly with the sidebar service's unit name, creating a new pane for that specific service.

`App::split_pane(direction)` does the following:
1. Gets the currently selected service name from the sidebar (`selected_unit_name()`).
2. Calls `PaneTree::split(focused_pane, direction, service_name, priority)`.
3. On success, updates `focused_pane` to the newly created pane's ID.
4. If the service name is non-empty, starts a journal stream for the new pane via `start_journal_for_pane()`.

`PaneTree::split()` delegates to `find_and_split()`, a recursive function that:
1. Checks `contains_leaf(node, target_id)` to confirm the target exists in this subtree.
2. When the matching leaf is found, uses `std::mem::replace` with a dummy leaf to take ownership of the original node.
3. Replaces the node in-place with a new `PaneNode::Split(PaneSplit { direction, ratio: 0.5, children: [original, new_leaf] })`.
4. Increments `next_id` and returns `Some(new_id)`.

### Closing a Pane

There are three entry paths:

1. **Keyboard**: User presses `w` (mapped to `KeyAction::ClosePane`). The app computes the next leaf ID first, then calls `PaneTree::close(old_id)`. On success, `focused_pane` is updated to the next leaf.

2. **Middle mouse click**: Detected via `MouseEventKind::Down(MouseButton::Middle)` in `handle_mouse()`. Hit-tests to find the pane under the cursor, then closes it the same way.

3. **Context menu**: "Close Pane" action in the pane context menu.

`PaneTree::close(target_id)` returns `false` (no-op) if the root is a `Leaf` (last pane protection). Otherwise it delegates to `close_node()`, which:
1. Checks if either immediate child of a `Split` is a leaf matching `target_id`.
2. If found, aborts the target leaf's journal handle (if any), replaces the parent `Split` node with the surviving sibling (sibling promotion).
3. If not an immediate child, recurses into both children.

### Layout Computation

`PaneTree::layout(area: Rect) -> Vec<(PaneId, Rect)>` recursively walks the tree via `layout_node()`:

- **Leaf**: Pushes `(leaf.id, area)` into the result vector.
- **Split(Horizontal)**: Computes `left_width = (area.width * ratio) as u16`, `right_width = area.width - left_width` (using `saturating_sub`). Recurses into left child with the left rect, right child with the right rect.
- **Split(Vertical)**: Same logic but splits `area.height` into `top_height` and `bottom_height`.

The result is consumed by `ui::render()` which iterates over each `(pane_id, rect)` pair, looks up the corresponding `PaneLeaf`, and calls `logs::render_log_pane()`. The result is also stored in `LayoutCache::pane_rects` for mouse hit-testing.

### Focus Cycling

`PaneTree::next_leaf_id(current: PaneId) -> PaneId`:
1. Calls `leaf_ids()` which performs a left-to-right in-order traversal via `collect_leaf_ids()`, collecting all leaf IDs into a `Vec`.
2. Finds the position of `current` in the list.
3. Returns `ids[(pos + 1) % ids.len()]` (wraps around).

Triggered by:
- **Tab key** (mapped to `KeyAction::CycleFocus`): Sets `focused_pane = pane_tree.next_leaf_id(focused_pane)`.
- Focus is also implicitly set when splitting (new pane gets focus) or closing (next pane gets focus).

### Mouse Focus

- **Left click on a pane**: `handle_mouse()` calls `hit_test(col, row)` which iterates over `layout_cache.pane_rects` to find which pane rect contains the click position. Returns `HitTarget::Pane(pane_id)`, and the handler sets `focused_pane = pane_id`.
- **Scroll wheel on a pane**: Hit-tests to find the pane, then adjusts that pane's `scroll_offset` (up adds 3, down subtracts 3).

### Per-Pane State (PaneLeaf)

Each `PaneLeaf` stores:

| Field | Type | Purpose |
|---|---|---|
| `id` | `PaneId` (`u64`) | Unique identifier, assigned from `PaneTree::next_id` |
| `service_name` | `String` | The systemd unit whose logs this pane displays |
| `log_buffer` | `VecDeque<String>` | Ring buffer of log lines, capped at `MAX_LOG_LINES` (10,000) |
| `scroll_offset` | `usize` | 0 means "follow/tail mode"; >0 means scrolled back by N lines |
| `search_query` | `String` | Active search filter text for highlighting matches |
| `priority_filter` | `Priority` | Journal priority level filter |
| `journal_handle` | `Option<JoinHandle<()>>` | Handle to the async journal streaming task; aborted on close or service change |

`push_line()` enforces the buffer cap: when at capacity, it pops from the front and adjusts `scroll_offset` downward (so the user's view position remains stable relative to content).

`is_following()` returns `true` when `scroll_offset == 0`, indicating the pane auto-scrolls to show the latest lines.

---

## Related Files

| File | Role | Key Exports/Interfaces |
|---|---|---|
| `src/ui/panes.rs` | Core data structures and tree operations | `PaneId`, `PaneTree`, `PaneNode`, `PaneLeaf`, `PaneSplit`, `SplitDirection`, `MAX_LOG_LINES` |
| `src/app.rs` | Application state and event handling for pane operations | `App::split_pane()`, `App::handle_key()` (SplitPrompt mode + Normal mode pane actions), `App::handle_mouse()` (click/middle-click/scroll on panes), `App::hit_test()`, `App::open_pane_context_menu()`, `App::execute_context_menu_action()`, `App::start_journal_for_pane()`, `App::handle_log_line()`, `App::handle_log_stream_ended()`, `InputMode::SplitPrompt`, `HitTarget::Pane` |
| `src/ui/mod.rs` | Top-level rendering; computes pane layout and renders each pane | `render()` (calls `pane_tree.layout()`, iterates results to render), `render_split_prompt()`, `LayoutCache` (stores `pane_rects` for hit-testing) |
| `src/ui/logs.rs` | Renders a single log pane widget | `render_log_pane(frame, rect, pane, focused)` |
| `src/ui/context_menu.rs` | Context menu types and rendering for pane actions | `ContextMenuAction::{SplitHorizontal, SplitVertical, SplitNewPaneHorizontal, SplitNewPaneVertical, ClosePane}`, `ContextMenuTarget::Pane`, `ContextMenu`, `render_context_menu()` |
| `src/ui/help.rs` | Help bar and overlay showing pane keybindings | References `KeyAction::PinPane`, `KeyAction::ClosePane`, `KeyAction::CycleFocus` |
| `src/config/keys.rs` | Keybinding definitions and defaults for pane actions | `KeyAction::PinPane` (default `p`), `KeyAction::ClosePane` (default `w`), `KeyAction::CycleFocus` (default `Tab`) |

---

## Invariants and Constraints

1. **At least one pane must exist at all times.** `PaneTree::close()` returns `false` and does nothing when the root is a `Leaf` (i.e., only one pane remains).

2. **Each PaneLeaf has a unique PaneId.** IDs are assigned from `PaneTree::next_id`, which is a monotonically increasing `u64` counter. IDs are never reused.

3. **`focused_pane` must always reference an existing leaf.** After every close or split operation, `focused_pane` is updated to point to either the newly created pane (on split) or the next sibling (on close, via `next_leaf_id()`). The single-pane guard in `close()` prevents the focused pane from being removed when it is the last one.

4. **Split nodes always have exactly two children.** The `PaneSplit::children` field is a fixed-size array `[Box<PaneNode>; 2]`. When a child is closed, the parent `Split` node is replaced entirely by the surviving sibling (sibling promotion), so no `Split` node ever has fewer than two children.

5. **Layout computation must fit within the available Rect without overflow.** The layout uses `saturating_sub` when computing the second child's dimension (`right_width = area.width.saturating_sub(left_width)`, `bottom_height = area.height.saturating_sub(top_height)`) to prevent underflow. Child rects are positioned at exact offsets within the parent rect, ensuring no overlap and no exceeding of bounds.

6. **Log buffer size is bounded.** `PaneLeaf::log_buffer` is capped at `MAX_LOG_LINES` (10,000). `push_line()` evicts the oldest entry when at capacity and adjusts `scroll_offset` to maintain view stability.

7. **Journal handles are cleaned up on pane close.** `close_node()` aborts the journal stream's `JoinHandle` before removing the leaf. `start_journal_for_pane()` also aborts any existing handle before starting a new stream.

8. **Split ratio is always 0.5.** Currently all splits divide space equally. The `ratio` field exists on `PaneSplit` to support future unequal splits, but it is always set to `0.5` by `find_and_split()`.

9. **Leaf traversal order is deterministic.** `collect_leaf_ids()` always traverses left child before right child, producing a stable ordering for focus cycling. This means Tab always moves focus left-to-right, top-to-bottom through the tree.
