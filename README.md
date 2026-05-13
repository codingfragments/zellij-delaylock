# delayLock

A [Zellij](https://zellij.dev) plugin that auto-returns your session to `Locked`
mode after a short idle timeout. It exists as a safety net for **leader-key
mistakes**: you press the leader to enter `Tmux` (or `Normal`) mode, miss the
follow-up action, and now Zellij is silently eating your keystrokes. delayLock
snaps the session back to `Locked` after a couple of seconds so your typing
goes back to the underlying terminal.

## How it works

Three rules, one timer:

1. **On any mode change**, if you've just left the target mode and entered a
   mode in the *active list* (default: `Normal`, `Tmux`), a timer starts
   (default: 2 seconds).
2. **On any keystroke** (anywhere in the Zellij session), the timer is reset
   to a fresh full interval — so as long as you're typing actions, you stay
   in the unlocked mode.
3. **When the timer expires** with no input since it was armed, the plugin
   calls `switch_to_input_mode(Locked)` and you're back in `Locked` mode.

Modes that are *not* in the active list (e.g. `Scroll`, `Search`, `Prompt`,
`RenameTab`) are left alone — those are modes where you legitimately want to
sit idle for a while.

On first load the plugin switches the session to the target mode immediately,
so you start locked.

### Why a timer that "re-arms" instead of resetting on each keypress

Zellij's plugin API does not let you cancel a `set_timeout`. delayLock instead
counts input events and snapshots the count when arming. When the timer fires,
it compares: if the count moved, the user was typing, so it re-arms with a
fresh full interval. If the count is unchanged, the user really was idle, and
it locks. The user-visible effect is "every keystroke gives you a fresh full
timeout", which is the right UX for a 2-second leader-key safety net.

## Requirements

- Zellij (any recent version — tested against `zellij-tile = 0.44`).
- Rust toolchain with the `wasm32-wasip1` target:
  ```fish
  rustup target add wasm32-wasip1
  ```

(Older Zellij documentation says `wasm32-wasi`. That target name was removed
in Rust 1.84; `wasm32-wasip1` is the same ABI under the new name. Zellij
loads either fine.)

## Build

```fish
git clone <this repo> delaylock
cd delaylock
cargo build --release
```

The artifact lands at `target/wasm32-wasip1/release/delaylock.wasm`. Copy it
somewhere stable — e.g.:

```fish
mkdir -p ~/.config/zellij/plugins
cp target/wasm32-wasip1/release/delaylock.wasm ~/.config/zellij/plugins/
```

## Install in your Zellij config

delayLock has no UI — it just needs to be running. The right place for that
is your `~/.config/zellij/config.kdl`, not a layout. Layouts are
session-scoped (you'd have to add it to every layout) and require a pane to
host the plugin; the config-level `load_plugins` block runs the plugin
**headless** in the background, automatically, for every session.

Add an alias in the `plugins` block (with your config), then reference it in
`load_plugins`:

```kdl
plugins {
    // ... any existing aliases ...
    delaylock location="file:~/.config/zellij/plugins/delaylock.wasm" {
        timeout_seconds "2.0"
        target_mode "locked"
        active_modes "normal,tmux"
    }
}

load_plugins {
    "delaylock"
}
```

That's it. The next time you start a Zellij session, delayLock loads in the
background. The first time, Zellij prompts once to grant
`ReadApplicationState` and `ChangeApplicationState` — accept both. After
that it's silent.

### Alternatives

- **Load ad-hoc** (e.g. to try it without editing config):
  ```fish
  zellij action launch-or-focus-plugin file:~/.config/zellij/plugins/delaylock.wasm --floating
  ```
- **Load via a layout** — only useful if you specifically want delayLock
  active in *some* layouts but not others. Same `plugin { ... }` block as
  above, nested inside a `pane { }` or `floating_panes { }`.

## Configuration

All settings go in the `plugin { ... }` block in the KDL.

| Key               | Default        | Meaning                                                                    |
|-------------------|----------------|----------------------------------------------------------------------------|
| `timeout_seconds` | `2.0`          | How long to wait, in seconds. Float. Must be positive.                     |
| `target_mode`     | `locked`       | The mode to return to. Any `InputMode` name, case-insensitive.             |
| `active_modes`    | `normal,tmux`  | Comma-separated list of modes that should auto-return. Case-insensitive.   |
| `logging`         | `false`        | If `true`, emit verbose state-machine logs. Accepts `true/false/yes/no/1/0/on/off`. |

Valid mode names: `normal`, `locked`, `resize`, `pane`, `tab`, `scroll`,
`entersearch`, `search`, `renametab`, `renamepane`, `session`, `move`,
`prompt`, `tmux`.

Unknown names are silently dropped. An empty (or all-unknown) `active_modes`
falls back to the default — a typo won't accidentally disable the plugin.

### Debugging with `logging`

Set `logging "true"` in the plugin config, then tail the Zellij log.

#### Where is the log file?

Zellij writes one log file per UID under its temp directory:

| Platform | Path |
|---|---|
| macOS   | `$TMPDIR/zellij-<uid>/zellij-log/zellij.log` |
| Linux   | `/tmp/zellij-<uid>/zellij-log/zellij.log` |

`<uid>` is your numeric user id (`id -u`). On macOS `$TMPDIR` is something
like `/var/folders/.../T/`; on Linux it's usually unset and `/tmp` is used.

If you're unsure, just locate it:

```fish
find $TMPDIR /tmp -name zellij.log 2>/dev/null
```

#### Tail it

The path expands at the shell, so the exact command depends on which shell
you're in.

**fish** (note: no `$` before the subshell):

```fish
tail -f $TMPDIR/zellij-(id -u)/zellij-log/zellij.log | grep delaylock
```

**bash / zsh**:

```bash
tail -f "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | grep delaylock
```

On Linux, replace `$TMPDIR` with `/tmp` in either shell.

#### What you'll see

Lines prefixed with `[delaylock]` covering: config loaded, permission
grant, every `ModeUpdate`, every `InputReceived`, every timer fire, and
every forced switch.

Turn it off (`logging "false"` or omit the key) for normal use — when
disabled, the logging code paths don't even format their arguments.

### Recommended setups

**Strict leader-key safety net** (the default):
```kdl
timeout_seconds "2.0"
target_mode "locked"
active_modes "normal,tmux"
```

**More forgiving** — give yourself longer to think:
```kdl
timeout_seconds "5.0"
target_mode "locked"
active_modes "normal,tmux"
```

**Auto-return to Normal instead of Locked** (less aggressive — your shell
keys are still eaten by Zellij when Normal is the target, so this only makes
sense if Locked isn't your usual base mode):
```kdl
timeout_seconds "3.0"
target_mode "normal"
active_modes "tmux,pane,tab,resize,move,session"
```

## Development

A dev layout is included: `zellij.kdl`. It opens the source file and loads the
plugin from the `debug/` build directory, so you can hot-reload as you edit.

```fish
zellij -l zellij.kdl
# in another shell, after each edit:
cargo build && zellij action start-or-reload-plugin file:target/wasm32-wasip1/debug/delaylock.wasm
```

`CLAUDE.md` has notes on the internals (state machine, event scoping, why
certain events were chosen over others) for anyone — human or AI — who wants
to extend the plugin.

## License

TBD.
