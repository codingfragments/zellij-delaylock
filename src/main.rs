use std::collections::{BTreeMap, HashSet};
use zellij_tile::prelude::*;

const DEFAULT_TIMEOUT_SECS: f64 = 2.0;
const DEFAULT_TARGET_MODE: InputMode = InputMode::Locked;

macro_rules! log {
    ($self:ident, $($arg:tt)*) => {
        if $self.logging {
            eprintln!("[delaylock] {}", format_args!($($arg)*));
        }
    };
}

struct State {
    timeout_secs: f64,
    target_mode: InputMode,
    active_modes: HashSet<InputMode>,
    logging: bool,
    current_mode: Option<InputMode>,
    permission_granted: bool,
    initial_lock_done: bool,
    input_count: u64,
    armed_snapshot: Option<u64>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            target_mode: DEFAULT_TARGET_MODE,
            active_modes: default_active_modes(),
            logging: false,
            current_mode: None,
            permission_granted: false,
            initial_lock_done: false,
            input_count: 0,
            armed_snapshot: None,
        }
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => Some(true),
        "false" | "no" | "0" | "off" => Some(false),
        _ => None,
    }
}

fn default_active_modes() -> HashSet<InputMode> {
    let mut s = HashSet::new();
    s.insert(InputMode::Normal);
    s.insert(InputMode::Tmux);
    s
}

fn parse_mode(name: &str) -> Option<InputMode> {
    match name.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(InputMode::Normal),
        "locked" => Some(InputMode::Locked),
        "resize" => Some(InputMode::Resize),
        "pane" => Some(InputMode::Pane),
        "tab" => Some(InputMode::Tab),
        "scroll" => Some(InputMode::Scroll),
        "entersearch" | "enter_search" => Some(InputMode::EnterSearch),
        "search" => Some(InputMode::Search),
        "renametab" | "rename_tab" => Some(InputMode::RenameTab),
        "renamepane" | "rename_pane" => Some(InputMode::RenamePane),
        "session" => Some(InputMode::Session),
        "move" => Some(InputMode::Move),
        "prompt" => Some(InputMode::Prompt),
        "tmux" => Some(InputMode::Tmux),
        _ => None,
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        if let Some(v) = configuration.get("logging") {
            if let Some(b) = parse_bool(v) {
                self.logging = b;
            }
        }
        if let Some(v) = configuration.get("timeout_seconds") {
            if let Ok(n) = v.trim().parse::<f64>() {
                if n.is_finite() && n > 0.0 {
                    self.timeout_secs = n;
                }
            }
        }
        if let Some(v) = configuration.get("target_mode") {
            if let Some(m) = parse_mode(v) {
                self.target_mode = m;
            }
        }
        if let Some(v) = configuration.get("active_modes") {
            let parsed: HashSet<InputMode> = v.split(',').filter_map(parse_mode).collect();
            if !parsed.is_empty() {
                self.active_modes = parsed;
            }
        }

        log!(self,
            "loaded: timeout={}s target={:?} active={:?}",
            self.timeout_secs,
            self.target_mode,
            self.active_modes
        );

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        subscribe(&[
            EventType::ModeUpdate,
            EventType::InputReceived,
            EventType::Timer,
            EventType::PermissionRequestResult,
        ]);
        log!(self, "requested permissions + subscribed to ModeUpdate/InputReceived/Timer/PermissionRequestResult");
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                log!(self, "permission GRANTED");
                self.permission_granted = true;
                self.lock_initial_if_needed();
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                log!(self, "permission DENIED — plugin will not be able to switch modes");
                self.permission_granted = false;
            }
            Event::ModeUpdate(info) => {
                let new_mode = info.mode;
                let prev = self.current_mode;
                self.current_mode = Some(new_mode);
                log!(self, "ModeUpdate: {:?} -> {:?}", prev, new_mode);
                self.lock_initial_if_needed();

                if new_mode == self.target_mode {
                    if self.armed_snapshot.is_some() {
                        log!(self, "  disarming (reached target mode {:?})", self.target_mode);
                    }
                    self.armed_snapshot = None;
                } else if self.active_modes.contains(&new_mode) {
                    log!(self, "  mode is in active list — arming timer");
                    self.arm_timer();
                } else {
                    if self.armed_snapshot.is_some() {
                        log!(self, "  disarming (mode {:?} not in active list)", new_mode);
                    }
                    self.armed_snapshot = None;
                }
            }
            Event::InputReceived => {
                self.input_count = self.input_count.wrapping_add(1);
                if let Some(snapshot) = self.armed_snapshot {
                    log!(self,
                        "InputReceived (count={}, snapshot={}, armed)",
                        self.input_count,
                        snapshot
                    );
                } else {
                    log!(self, "InputReceived (count={}, idle)", self.input_count);
                }
            }
            Event::Timer(secs) => {
                log!(self, "Timer fired ({}s elapsed)", secs);
                self.handle_timer();
            }
            _ => {}
        }
        false
    }

    fn pipe(&mut self, _pipe_message: PipeMessage) -> bool {
        false
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn lock_initial_if_needed(&mut self) {
        if self.initial_lock_done || !self.permission_granted {
            return;
        }
        if self.current_mode == Some(self.target_mode) {
            log!(self, "initial lock: already in {:?}, nothing to do", self.target_mode);
            self.initial_lock_done = true;
            return;
        }
        log!(self, "initial lock: switching to {:?}", self.target_mode);
        switch_to_input_mode(&self.target_mode);
        self.initial_lock_done = true;
    }

    fn arm_timer(&mut self) {
        self.armed_snapshot = Some(self.input_count);
        set_timeout(self.timeout_secs);
        log!(self,
            "  arm_timer: snapshot={}, set_timeout({}s)",
            self.input_count,
            self.timeout_secs
        );
    }

    fn handle_timer(&mut self) {
        let Some(snapshot) = self.armed_snapshot else {
            log!(self, "  handle_timer: not armed, ignoring");
            return;
        };
        let current = match self.current_mode {
            Some(m) => m,
            None => {
                log!(self, "  handle_timer: current_mode unknown, dropping armed state");
                self.armed_snapshot = None;
                return;
            }
        };
        if current == self.target_mode {
            log!(self,
                "  handle_timer: current mode is target ({:?}), nothing to do",
                current
            );
            self.armed_snapshot = None;
            return;
        }
        if !self.active_modes.contains(&current) {
            log!(self,
                "  handle_timer: current mode {:?} not in active list, dropping",
                current
            );
            self.armed_snapshot = None;
            return;
        }
        if self.input_count != snapshot {
            log!(self,
                "  handle_timer: input observed (snapshot={}, now={}), re-arming",
                snapshot,
                self.input_count
            );
            self.arm_timer();
            return;
        }
        log!(self,
            "  handle_timer: idle (count still {}), FORCING switch_to_input_mode({:?})",
            snapshot,
            self.target_mode
        );
        switch_to_input_mode(&self.target_mode);
        self.armed_snapshot = None;
    }
}
