# sysdui

A terminal UI for managing systemd services. Browse services, watch live logs, and control service lifecycle — all from one screen.

<img src="https://private-user-images.githubusercontent.com/82485126/567146541-fb859245-7ff4-4ebd-97a9-1f1a802d787f.png?jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJnaXRodWIuY29tIiwiYXVkIjoicmF3LmdpdGh1YnVzZXJjb250ZW50LmNvbSIsImtleSI6ImtleTUiLCJleHAiOjE3NzQwNTI2MTcsIm5iZiI6MTc3NDA1MjMxNywicGF0aCI6Ii84MjQ4NTEyNi81NjcxNDY1NDEtZmI4NTkyNDUtN2ZmNC00ZWJkLTk3YTktMWYxYTgwMmQ3ODdmLnBuZz9YLUFtei1BbGdvcml0aG09QVdTNC1ITUFDLVNIQTI1NiZYLUFtei1DcmVkZW50aWFsPUFLSUFWQ09EWUxTQTUzUFFLNFpBJTJGMjAyNjAzMjElMkZ1cy1lYXN0LTElMkZzMyUyRmF3czRfcmVxdWVzdCZYLUFtei1EYXRlPTIwMjYwMzIxVDAwMTgzN1omWC1BbXotRXhwaXJlcz0zMDAmWC1BbXotU2lnbmF0dXJlPWExYjg2MGNjNmU0Yzk3NjkwNjM2NWQyYmE1YzBlMjY5ZjYyYzUwNDMzZTNkY2RmMGJiMTI2MDYyMDgzYTZkNzYmWC1BbXotU2lnbmVkSGVhZGVycz1ob3N0In0.XOeVB54sJ4qzXrf2ARis_SopA5b6rnNLo-vFh6QA5MY">

## Install

Requires Rust 1.85+ (edition 2024).

```sh
cargo install --path .
```

## Usage

```sh
sysdui
```

Run as a normal user. When you perform an action on a system service (start, stop, etc.), `sudo` is invoked automatically — you'll see the password prompt in your terminal, then the TUI resumes. User services don't need elevation.

Logs are written to `~/.local/share/sysdui/sysdui.log`. Set `RUST_LOG=sysdui=debug` for verbose output.

## Keybindings

Press `?` at any time to see the full list. All bindings are single lowercase keys — no shift combinations.

### Navigation

| Key | Action |
|-----|--------|
| `j` / `k` / arrows | Move up/down in service list |
| `Enter` | Select service (loads details + logs) |
| `PgUp` / `PgDn` | Scroll logs |
| `Tab` | Cycle focus between panes |

### Service Actions

| Key | Action |
|-----|--------|
| `s` | Start |
| `r` | Restart |
| `x` | Stop |
| `n` | Enable |
| `d` | Disable |
| `o` | Daemon reload |
| `e` | Edit unit file in `$EDITOR` |

Each action shows a confirmation prompt. Press `y` to confirm or any other key to cancel.

### Search & Filter

| Key | Action |
|-----|--------|
| `/` | Fuzzy search services by name |
| `Ctrl-/` | Search within log output |
| `f` | Cycle scope filter: User / System / Both |
| `i` | Toggle include/all mode |
| `t` | Cycle sort: Name / Status / Enabled |
| `l` | Cycle log priority: err / warning / notice / info / debug |

### Pane Management

Split the log area into multiple panes, each pinned to a different service — useful for watching related services side by side.

| Key | Action |
|-----|--------|
| `p` | Split focused pane (then `h` for horizontal, `v` for vertical) |
| `w` | Close focused pane |
| `Tab` | Cycle focus between panes |

### Mouse

| Action | Effect |
|--------|--------|
| Click service | Select it in sidebar |
| Double-click service | Select + load logs into focused pane |
| Right-click service | Context menu: Start / Stop / Restart / Enable / Disable / Split into pane |
| Click log pane | Focus that pane |
| Right-click log pane | Context menu: Split Horizontal / Vertical / Close |
| Middle-click log pane | Close pane |
| Scroll wheel over sidebar | Move selection up/down |
| Scroll wheel over log pane | Scroll that pane's logs (even if unfocused) |
| Click status bar | Cycle the clicked filter/sort (Scope / Status / Mode / Sort) |

### General

| Key | Action |
|-----|--------|
| `q` | Quit |
| `?` | Show full keybinding help |
| `Esc` | Cancel / close overlay |

## Features

- **Service list** with color coding: green = active, red = failed, yellow = transitioning, gray = inactive. Failed services are always pinned to the top.
- **Fuzzy search** powered by nucleo — type a few characters of a service name to filter instantly.
- **Live log tail** streams `journalctl` output in real-time. Scroll up to freeze the view; a "new lines" counter appears so you don't lose your place.
- **Detail panel** shows status, PID, memory usage, uptime, unit file path, enabled state, and dependencies — updated in real-time via D-Bus signals.
- **Pane splitting** lets you watch logs from multiple services simultaneously in a tmux-like layout.
- **Privilege escalation** is handled transparently — the TUI suspends, runs `sudo systemctl`, and resumes. `sudo`'s credential cache means you typically only authenticate once.
- **User + system services** are shown together (filterable). User services use `systemctl --user` and need no elevation.

## Configuration

Optional. Place a file at `~/.config/sysdui/config.toml`:

```toml
[filter]
mode = "all"         # "all" (show everything except excludes) or "include" (only show includes)
show = "both"        # "user", "system", or "both"
include = ["nginx.service", "sshd.service"]
exclude = ["systemd-tmpfiles-clean.service"]

[sort]
default = "name"     # "name", "status", or "enabled"

[log]
priority = "info"    # "err", "warning", "notice", "info", "debug"

[confirmations]
global = true        # set to false to skip all confirmation prompts
start = true
stop = true
restart = true
enable = true
disable = true
daemon_reload = true

[keys]
# Remap any binding. Value is a key combo string.
# Examples: "s", "ctrl-s", "f1", "enter"
start = "s"
stop = "x"
quit = "q"
# ... see source for all key names
```

## Requirements

- Linux with systemd
- D-Bus (system and session bus)
- `journalctl` in `$PATH`
- `sudo` for system service actions
