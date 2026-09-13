use super::stage::STAGES;
use crate::config::Config;
use crate::stages::kernel::FEATURE_PACKS;
use crate::stages::usb::Device;
use anyhow::Result;
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};

const LOG_CAP: usize = 2000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Running,
    Success,
    Failed,
}

pub struct StageState {
    pub status: Status,
    pub log: VecDeque<String>,
}

impl Default for StageState {
    fn default() -> Self {
        StageState {
            status: Status::Idle,
            log: VecDeque::new(),
        }
    }
}

pub enum Screen {
    Dashboard,
    DevicePicker { devices: Vec<Device>, selected: usize, error: Option<String> },
    ConfirmWrite { device: Device, typed: String },
    ConfirmClean { idx: usize },
    Settings { selected: usize },
    EditHostname { typed: String },
    FilePicker { dir: PathBuf, entries: Vec<PickerEntry>, selected: usize, error: Option<String> },
}

pub struct PickerEntry {
    pub name: String,
    pub is_dir: bool,
}

/// One row of the Screen::Settings overlay, in display order.
#[derive(Clone, Copy)]
pub enum SettingsRow {
    Networking,
    ForceRebuild,
    Hostname,
    /// Index into `FEATURE_PACKS`.
    Feature(usize),
    /// Shown directly under the `boot-logo` feature row.
    CustomLogo,
}

/// The Screen::Settings overlay's rows, in display/selection order. Built
/// fresh each time (cheap: FEATURE_PACKS is tiny) rather than cached, so
/// there's one source of truth for row order, indices, and row count.
pub fn settings_rows() -> Vec<SettingsRow> {
    let mut rows = vec![SettingsRow::Networking, SettingsRow::ForceRebuild, SettingsRow::Hostname];
    for (i, pack) in FEATURE_PACKS.iter().enumerate() {
        rows.push(SettingsRow::Feature(i));
        if pack.key == "boot-logo" {
            rows.push(SettingsRow::CustomLogo);
        }
    }
    rows
}

pub enum AppEvent {
    Log(usize, String),
    Done(usize, bool),
}

pub struct App {
    pub config_path: PathBuf,
    pub cfg: Config,
    /// Pass --force to the next stage run. Session-only, not persisted:
    /// a one-shot "rebuild even if already built" switch, distinct from
    /// `cfg.networking` and friends which are persisted build settings.
    pub force: bool,
    pub stages: Vec<StageState>,
    pub selected: usize,
    pub screen: Screen,
    pub running: Option<usize>,
    pub should_quit: bool,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
}

impl App {
    pub fn new(config_path: PathBuf) -> Result<Self> {
        let cfg = Config::load(&config_path)?;
        let (tx, rx) = mpsc::channel();
        Ok(App {
            config_path,
            cfg,
            force: false,
            stages: STAGES.iter().map(|_| StageState::default()).collect(),
            selected: 0,
            screen: Screen::Dashboard,
            running: None,
            should_quit: false,
            tx,
            rx,
        })
    }

    pub fn toggle_networking(&mut self) -> Result<()> {
        self.cfg.networking = !self.cfg.networking;
        self.cfg.save(&self.config_path)
    }

    pub fn is_feature_enabled(&self, key: &str) -> bool {
        self.cfg.kernel.features.iter().any(|f| f == key)
    }

    pub fn toggle_feature(&mut self, key: &str) -> Result<()> {
        let features = &mut self.cfg.kernel.features;
        if let Some(pos) = features.iter().position(|f| f == key) {
            features.remove(pos);
        } else {
            features.push(key.to_string());
        }
        self.cfg.save(&self.config_path)
    }

    /// Sets `kernel.logo_file` and persists it.
    pub fn set_logo_file(&mut self, path: PathBuf) -> Result<()> {
        self.cfg.kernel.logo_file = Some(path);
        self.cfg.save(&self.config_path)
    }

    /// Opens the file picker for choosing `kernel.logo_file`, starting in
    /// the current logo file's directory if one is set, else the current
    /// working directory.
    pub fn open_file_picker(&mut self) {
        let start_dir = self
            .cfg
            .kernel
            .logo_file
            .as_ref()
            .and_then(|p| p.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        self.screen = read_dir_screen(start_dir);
    }

    /// Navigates the open file picker into `dir`.
    pub fn navigate_picker(&mut self, dir: PathBuf) {
        self.screen = read_dir_screen(dir);
    }

    /// Sets `image.hostname` and persists it. A blank value is ignored
    /// rather than shipping an empty hostname.
    pub fn set_hostname(&mut self, hostname: &str) -> Result<()> {
        let hostname = hostname.trim();
        if hostname.is_empty() {
            return Ok(());
        }
        self.cfg.image.hostname = hostname.to_string();
        self.cfg.save(&self.config_path)
    }

    /// Drains any pending events from running child processes. Returns true
    /// if anything changed (so the caller knows to redraw).
    pub fn poll_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            changed = true;
            match event {
                AppEvent::Log(idx, line) => {
                    let log = &mut self.stages[idx].log;
                    log.push_back(line);
                    while log.len() > LOG_CAP {
                        log.pop_front();
                    }
                }
                AppEvent::Done(idx, success) => {
                    self.stages[idx].status = if success { Status::Success } else { Status::Failed };
                    self.running = None;
                }
            }
        }
        changed
    }

    pub fn open_device_picker(&mut self) {
        match crate::stages::usb::list_removable_devices() {
            Ok(devices) => {
                self.screen = Screen::DevicePicker { devices, selected: 0, error: None };
            }
            Err(e) => {
                self.screen = Screen::DevicePicker {
                    devices: vec![],
                    selected: 0,
                    error: Some(e.to_string()),
                };
            }
        }
    }

    /// Removes the given stage's on-disk outputs so it (and anything
    /// downstream) redoes its work next run.
    pub fn clean_stage(&mut self, idx: usize) {
        if self.running.is_some() {
            return;
        }
        let kind = STAGES[idx];
        self.stages[idx].log.clear();
        match kind.clean(&self.cfg) {
            Ok(()) => {
                self.stages[idx].log.push_back("cleaned".to_string());
                self.stages[idx].status = Status::Idle;
            }
            Err(e) => {
                self.stages[idx].log.push_back(format!("clean failed: {e}"));
                self.stages[idx].status = Status::Failed;
            }
        }
    }

    pub fn run_stage(&mut self, idx: usize, device: Option<&str>) {
        if self.running.is_some() {
            return;
        }
        self.running = Some(idx);
        self.stages[idx].status = Status::Running;
        self.stages[idx].log.clear();

        let kind = STAGES[idx];
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("linux-builder"));
        let mut args = vec!["--config".to_string(), self.config_path.to_string_lossy().to_string()];
        args.extend(kind.subcommand_args(device));
        if self.force {
            args.push("--force".to_string());
        }

        let child = Command::new(&exe)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child: Child = match child {
            Ok(c) => c,
            Err(e) => {
                self.stages[idx].log.push_back(format!("failed to spawn: {e}"));
                self.stages[idx].status = Status::Failed;
                self.running = None;
                return;
            }
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        if let Some(stdout) = stdout {
            spawn_reader(stdout, idx, self.tx.clone());
        }
        if let Some(stderr) = stderr {
            spawn_reader(stderr, idx, self.tx.clone());
        }

        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            let success = matches!(status, Ok(s) if s.success());
            let _ = tx.send(AppEvent::Done(idx, success));
        });
    }
}

/// Builds a `Screen::FilePicker` for `dir`, listing subdirectories and
/// `.ppm` files (plus a `..` entry, unless `dir` has no parent).
fn read_dir_screen(dir: PathBuf) -> Screen {
    match list_dir(&dir) {
        Ok(entries) => Screen::FilePicker { dir, entries, selected: 0, error: None },
        Err(e) => Screen::FilePicker { dir, entries: vec![], selected: 0, error: Some(e.to_string()) },
    }
}

fn list_dir(dir: &Path) -> Result<Vec<PickerEntry>> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            dirs.push(PickerEntry { name, is_dir: true });
        } else if name.to_lowercase().ends_with(".ppm") {
            files.push(PickerEntry { name, is_dir: false });
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));

    let mut entries = Vec::new();
    if dir.parent().is_some() {
        entries.push(PickerEntry { name: "..".to_string(), is_dir: true });
    }
    entries.extend(dirs);
    entries.extend(files);
    Ok(entries)
}

/// Reads raw bytes and splits on '\r' or '\n', so progress output that
/// overwrites a line in place (make, dd status=progress) still streams as
/// distinct log entries instead of arriving as one giant buffered line.
fn spawn_reader(mut reader: impl Read + Send + 'static, idx: usize, tx: Sender<AppEvent>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut pending = String::new();
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            pending.push_str(&String::from_utf8_lossy(&buf[..n]));
            while let Some(pos) = pending.find(['\r', '\n']) {
                let line = pending[..pos].to_string();
                pending.drain(..=pos);
                if !line.is_empty() {
                    let _ = tx.send(AppEvent::Log(idx, line));
                }
            }
        }
        if !pending.is_empty() {
            let _ = tx.send(AppEvent::Log(idx, pending));
        }
    });
}
