# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Zellij plugin (Rust → WASM) that auto-returns the session to `Locked` mode after a short idle timeout. Use case: protect against missed leader-key follow-ups. User unlocks (e.g. into `Tmux` mode), and if no action key follows within N seconds, the session snaps back to `Locked`.

## Build / dev loop

The plugin compiles to `target/wasm32-wasip1/(debug|release)/delaylock.wasm`.

```fish
cargo build              # debug — used by zellij.kdl
cargo build --release    # smaller artifact for distribution
```

Iterate inside Zellij with the bundled dev layout (loads the plugin as a floating pane):

```fish
zellij -l zellij.kdl
# after each cargo build:
zellij action start-or-reload-plugin file:target/wasm32-wasip1/debug/delaylock.wasm
```

The build target is **`wasm32-wasip1`**, not `wasm32-wasi`. The legacy name was removed in Rust 1.84; older Zellij docs/examples still say `wasm32-wasi` — the ABI is identical, only the target name changed.

## Architecture

Single-file plugin: `src/main.rs` defines `State`, implements `ZellijPlugin`, registered with `register_plugin!(State)`.

**Why these events and not others:**
- `Event::InputReceived` — session-wide ("input was received anywhere in the app"). This is what resets the idle timer.
- `Event::Key` is **pane-focused only** ("a key was pressed while the user is focused on this plugin's pane"), so it's useless for a session-wide idle detector. Do not subscribe to it.
- `Event::ModeUpdate(ModeInfo)` — session-wide, tells us when mode changes and what it is now. The only path to arming the timer.
- `Event::Timer(f64)` — fires after `set_timeout(secs)`. **Timers cannot be cancelled.** See state machine below for how we deal with this.
- `Event::PermissionRequestResult` — permission grant is async; we defer the initial switch-to-locked until we receive a grant.

**State machine (in `update`):**

1. On `PermissionRequestResult(Granted)` → flip flag, run `lock_initial_if_needed` (calls `switch_to_input_mode(&target_mode)` once).
2. On `ModeUpdate(info)`:
   - new mode == target → disarm
   - new mode ∈ `active_modes` → arm timer (snapshot `input_count`, `set_timeout(timeout_secs)`)
   - else → disarm (e.g. `Search`, `Scroll`, `Prompt` should not auto-return)
3. On `InputReceived` → `input_count += 1`. That's it — we don't re-arm here, we just bump the counter.
4. On `Timer(_)` →
   - if mode left active list or reached target → drop the snapshot, do nothing
   - if `input_count != snapshot` (user typed while timer ran) → re-arm with full `timeout_secs` and a fresh snapshot
   - else → `switch_to_input_mode(&target_mode)`

The **input-count snapshot trick** replaces what would normally be timer cancellation: instead of cancelling on each keypress (impossible), we let the timer fire, compare counters, and re-arm if the user was active. Side effect: a fresh keystroke gives the user the full `timeout_secs` again, not just the remainder — which is the right UX for a 2s leader-key safety net.

**Permissions requested:** `ReadApplicationState` (for `ModeUpdate`), `ChangeApplicationState` (for `switch_to_input_mode`).

## How users install it

Canonical install is the `plugins { }` + `load_plugins { }` blocks in
`~/.config/zellij/config.kdl` — runs headless in the background for every
session. The `zellij.kdl` in this repo loads the plugin as a floating pane
instead; that's intentional for dev (visible pane = visible `println!`
output, easy hot-reload target). Don't conflate the two — production users
should not use the dev layout's pattern.

## Config (passed in via the plugin alias's KDL block)

| key | default | format |
|---|---|---|
| `timeout_seconds` | `2.0` | positive float |
| `target_mode` | `locked` | any `InputMode` name, case-insensitive |
| `active_modes` | `normal,tmux` | comma-separated `InputMode` names |
| `logging` | `false` | `true/false/yes/no/1/0/on/off` |

The `log!` macro is gated on `self.logging` and uses `format_args!`, so when
disabled there is no allocation and no syscall — safe to leave call sites in
hot paths. Logs land in `$TMPDIR/zellij-<uid>/zellij-log/zellij.log` (macOS)
or `/tmp/zellij-<uid>/zellij-log/zellij.log` (Linux), prefixed `[delaylock]`.

`parse_mode` in `src/main.rs` is the single source of truth for accepted mode names — it must list every `InputMode` variant. If a future zellij-tile version adds a variant, update `parse_mode` too.

Unknown mode names are silently dropped. An empty/all-unknown `active_modes` falls back to the defaults; this is intentional so a typo doesn't disable the plugin entirely.

## Upstream docs

- Plugin lifecycle & trait: https://zellij.dev/documentation/plugin-lifecycle.html
- API reference: https://docs.rs/zellij-tile/latest/zellij_tile/
- `InputMode` variants and `Event` payloads: linked from the docs.rs page above
