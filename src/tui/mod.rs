mod app;
mod stage;

use crate::stages::kernel::FEATURE_PACKS;
use crate::stages::usb::Device;
use anyhow::Result;
use app::{settings_count, App, Screen, Status, FIXED_SETTINGS_COUNT};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;
use ratatui::Terminal;
use stage::STAGES;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

pub fn run(config_path: PathBuf) -> Result<()> {
    // Several stages need `sudo`. Spawned stages run with no stdin, so a
    // password prompt can't be answered from inside the dashboard; check
    // non-interactively up front (in the plain terminal) and, if that
    // fails, give the user a chance to authenticate here before the
    // alternate screen takes over.
    if std::process::Command::new("sudo").args(["-n", "true"]).status().map(|s| !s.success()).unwrap_or(true) {
        println!("Some stages need sudo (build-toolchain, make-image, write-usb).");
        println!("Authenticating now so those stages don't hang waiting for a password...");
        let _ = std::process::Command::new("sudo").arg("-v").status();
    }

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = App::new(config_path).and_then(|mut app| event_loop(&mut terminal, &mut app));

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        app.poll_events();
        terminal.draw(|f| draw(f, app))?;

        if app.should_quit {
            return Ok(());
        }

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            handle_key(app, key.code);
        }
    }
}

fn handle_key(app: &mut App, code: KeyCode) {
    match &mut app.screen {
        Screen::Dashboard => match code {
            KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                if app.selected > 0 {
                    app.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.selected + 1 < STAGES.len() {
                    app.selected += 1;
                }
            }
            KeyCode::Char('s') => app.screen = Screen::Settings { selected: 0 },
            KeyCode::Char('c') => {
                if app.running.is_none() {
                    let kind = STAGES[app.selected];
                    if kind.can_clean() && kind.is_present(&app.cfg) {
                        app.screen = Screen::ConfirmClean { idx: app.selected };
                    }
                }
            }
            KeyCode::Enter => {
                if app.running.is_some() {
                    return;
                }
                if STAGES[app.selected].needs_device() {
                    app.open_device_picker();
                } else {
                    app.run_stage(app.selected, None);
                }
            }
            _ => {}
        },
        Screen::Settings { selected } => match code {
            KeyCode::Esc | KeyCode::Char('s') => app.screen = Screen::Dashboard,
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected + 1 < settings_count() {
                    *selected += 1;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => match *selected {
                0 => {
                    // Best-effort: if the config file can't be written (e.g.
                    // permissions), just leave the in-memory toggle reverted
                    // rather than surfacing a save error into a stage's log.
                    if app.toggle_networking().is_err() {
                        app.cfg.networking = !app.cfg.networking;
                    }
                }
                1 => app.force = !app.force,
                i => {
                    let key = FEATURE_PACKS[i - FIXED_SETTINGS_COUNT].key;
                    if app.toggle_feature(key).is_err() {
                        // Best-effort revert, same as the networking toggle above.
                        if let Some(pos) = app.cfg.kernel.features.iter().position(|f| f == key) {
                            app.cfg.kernel.features.remove(pos);
                        } else {
                            app.cfg.kernel.features.push(key.to_string());
                        }
                    }
                }
            },
            _ => {}
        },
        Screen::ConfirmClean { idx } => match code {
            KeyCode::Esc | KeyCode::Char('n') => app.screen = Screen::Dashboard,
            KeyCode::Enter | KeyCode::Char('y') => {
                let idx = *idx;
                app.screen = Screen::Dashboard;
                app.clean_stage(idx);
            }
            _ => {}
        },
        Screen::DevicePicker { devices, selected, .. } => match code {
            KeyCode::Esc => app.screen = Screen::Dashboard,
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected + 1 < devices.len() {
                    *selected += 1;
                }
            }
            KeyCode::Char('r') => app.open_device_picker(),
            KeyCode::Enter => {
                if let Some(device) = devices.get(*selected).cloned() {
                    app.screen = Screen::ConfirmWrite { device, typed: String::new() };
                }
            }
            _ => {}
        },
        #[allow(clippy::collapsible_match)] // guard would need to run before the enum's fields are bound
        Screen::ConfirmWrite { device, typed } => match code {
            KeyCode::Esc => app.screen = Screen::Dashboard,
            KeyCode::Backspace => {
                typed.pop();
            }
            KeyCode::Char(c) => typed.push(c),
            KeyCode::Enter => {
                if *typed == device.path() {
                    let device_path = device.path();
                    app.screen = Screen::Dashboard;
                    let idx = STAGES.iter().position(|s| s.needs_device()).unwrap();
                    app.run_stage(idx, Some(&device_path));
                }
            }
            _ => {}
        },
    }
}

fn draw(f: &mut Frame, app: &App) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(chunks[0]);

    draw_stage_list(f, app, body[0]);
    draw_log_pane(f, app, body[1]);
    draw_help(f, app, chunks[1]);

    match &app.screen {
        Screen::DevicePicker { devices, selected, error } => {
            draw_device_picker(f, size, devices, *selected, error.as_deref())
        }
        Screen::ConfirmWrite { device, typed } => draw_confirm(f, size, device, typed),
        Screen::ConfirmClean { idx } => draw_confirm_clean(f, size, STAGES[*idx].label()),
        Screen::Settings { selected } => draw_settings(f, size, app, *selected),
        Screen::Dashboard => {}
    }
}

fn draw_stage_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = STAGES
        .iter()
        .enumerate()
        .map(|(i, stage)| {
            let state = &app.stages[i];
            let (icon, color) = match state.status {
                Status::Idle => ("  ", Color::Gray),
                Status::Running => (".. ", Color::Yellow),
                Status::Success => ("OK ", Color::Green),
                Status::Failed => ("!! ", Color::Red),
            };
            let present = if !stage.can_clean() {
                "  "
            } else if stage.is_present(&app.cfg) {
                "* "
            } else {
                "  "
            };
            let mut style = Style::default().fg(color);
            if i == app.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            ListItem::new(Line::from(Span::styled(format!("{icon}{present}{}", stage.label()), style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Pipeline stages (* = output present)"));
    f.render_widget(list, area);
}

fn draw_log_pane(f: &mut Frame, app: &App, area: Rect) {
    let state = &app.stages[app.selected];
    let height = area.height.saturating_sub(2) as usize;
    let start = state.log.len().saturating_sub(height);
    let lines: Vec<Line> = state.log.iter().skip(start).map(|l| Line::from(l.as_str())).collect();

    let title = format!("Log: {}", STAGES[app.selected].label());
    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(paragraph, area);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let text = if app.running.is_some() {
        "running... (q to quit once idle)".to_string()
    } else {
        format!(
            "up/down: select  enter: run  c: clean  s: settings  q/esc: quit{}",
            if app.force { "  [force: ON]" } else { "" }
        )
    };
    f.render_widget(Paragraph::new(text), area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn draw_device_picker(f: &mut Frame, area: Rect, devices: &[Device], selected: usize, error: Option<&str>) {
    let popup = centered_rect(70, 60, area);
    let items: Vec<ListItem> = if devices.is_empty() {
        vec![ListItem::new(error.unwrap_or("no removable disks found (press r to refresh)"))]
    } else {
        devices
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let label = format!(
                    "{}  {}  {}  {}",
                    d.path(),
                    d.size,
                    d.tran,
                    if d.model.is_empty() { "-" } else { &d.model }
                );
                let style = if i == selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect()
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Select a USB device (enter: pick, r: refresh, esc: cancel)"),
    );
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(list, popup);
}

fn draw_settings(f: &mut Frame, area: Rect, app: &App, selected: usize) {
    let popup = centered_rect(60, 30, area);
    let mut rows: Vec<(&str, bool, &str)> = vec![
        (
            "Networking",
            app.cfg.networking,
            "busybox udhcpc/ifconfig/route/ping, DHCP at boot, QEMU NIC",
        ),
        (
            "Force rebuild (this session)",
            app.force,
            "pass --force to the next stage you run",
        ),
    ];
    for pack in FEATURE_PACKS {
        rows.push((pack.label, app.is_feature_enabled(pack.key), pack.description));
    }

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (label, on, desc))| {
            let mark = if *on { "[x]" } else { "[ ]" };
            let style = if i == selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(format!("{mark} {label} — {desc}"), style)))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Settings (enter/space: toggle, esc: close)"),
    );
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(list, popup);
}

fn draw_confirm(f: &mut Frame, area: Rect, device: &Device, typed: &str) {
    let popup = centered_rect(60, 30, area);
    let text = vec![
        Line::from(format!(
            "About to PERMANENTLY ERASE {} ({}, {})",
            device.path(),
            device.size,
            if device.model.is_empty() { "-" } else { &device.model }
        )),
        Line::from(""),
        Line::from(format!("Type the device path to confirm: {typed}")),
        Line::from(""),
        Line::from("enter: confirm  esc: cancel"),
    ];
    let paragraph = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Confirm write"));
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(paragraph, popup);
}

fn draw_confirm_clean(f: &mut Frame, area: Rect, label: &str) {
    let popup = centered_rect(60, 20, area);
    let text = vec![
        Line::from(format!("Clean output for \"{label}\"?")),
        Line::from("This forces it (and anything downstream) to redo its work."),
        Line::from(""),
        Line::from("y/enter: confirm  n/esc: cancel"),
    ];
    let paragraph = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Confirm clean"));
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(paragraph, popup);
}
