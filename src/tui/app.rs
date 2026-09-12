use super::stage::STAGES;
use crate::stages::usb::Device;
use std::collections::VecDeque;
use std::io::Read;
use std::path::PathBuf;
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
}

pub enum AppEvent {
    Log(usize, String),
    Done(usize, bool),
}

pub struct App {
    pub config_path: PathBuf,
    pub stages: Vec<StageState>,
    pub selected: usize,
    pub screen: Screen,
    pub running: Option<usize>,
    pub should_quit: bool,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
}

impl App {
    pub fn new(config_path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        App {
            config_path,
            stages: STAGES.iter().map(|_| StageState::default()).collect(),
            selected: 0,
            screen: Screen::Dashboard,
            running: None,
            should_quit: false,
            tx,
            rx,
        }
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

        let mut command = if let Some(secs) = kind.timeout_secs() {
            let mut c = Command::new("timeout");
            c.arg(secs.to_string()).arg(&exe).args(&args);
            c
        } else {
            let mut c = Command::new(&exe);
            c.args(&args);
            c
        };

        let child = command
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
