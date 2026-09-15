use crate::app::{self, settings_rows, App, Focus, Screen, SettingsRow, Status};
use crate::registry::Registry;
use anyhow::Result;
use buildpack_core::pipeline::Device;
use buildpacks::kernel::FEATURE_PACKS;
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
use crate::stage::{StageKind, STAGES};
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

pub fn run(config_path: PathBuf, reg: Box<dyn Registry>) -> Result<()> {
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

    let result = App::new(config_path, reg).and_then(|mut app| event_loop(&mut terminal, &mut app));

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

/// Index of the first settings row matching `pred`, for returning to a
/// specific row (e.g. Hostname) after a sub-screen closes.
fn settings_row_index(pred: impl Fn(&SettingsRow) -> bool) -> usize {
    settings_rows().iter().position(pred).unwrap_or(0)
}

fn handle_key(app: &mut App, code: KeyCode) {
    match &mut app.screen {
        Screen::Dashboard => match app.focus {
            Focus::StageList => match code {
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
                // Only BuildUserland has anything on the right worth
                // moving into — every other stage's pane is just a log.
                KeyCode::Right | KeyCode::Char('l') => {
                    if STAGES[app.selected] == StageKind::BuildUserland {
                        let len = app.userland_progress().len();
                        if len > 0 {
                            app.package_selected = app.package_selected.min(len - 1);
                            app.focus = Focus::PackageList;
                        }
                    }
                }
                KeyCode::Char('s') => app.screen = Screen::Settings { selected: 0 },
                KeyCode::Char('c') => {
                    if app.running.is_none() {
                        let kind = STAGES[app.selected];
                        if kind.can_clean() && kind.is_present(&app.cfg, app.reg.as_ref()) {
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
                        app.run_stage(app.selected, None, false);
                    }
                }
                KeyCode::Char('f') => {
                    // Force-rebuild the selected stage. Not meaningful for
                    // WriteUsb (it has no "already done" check to bypass; it
                    // always re-confirms and re-writes), so it's a no-op there.
                    if app.running.is_none() && !STAGES[app.selected].needs_device() {
                        app.run_stage(app.selected, None, true);
                    }
                }
                _ => {}
            },
            // The package checklist on BuildUserland's own pane — enter/f
            // here rebuild just the one package under the cursor, not the
            // whole stage.
            Focus::PackageList => match code {
                KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                KeyCode::Left | KeyCode::Char('h') => app.focus = Focus::StageList,
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.package_selected > 0 {
                        app.package_selected -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.package_selected + 1 < app.userland_progress().len() {
                        app.package_selected += 1;
                    }
                }
                KeyCode::Enter => {
                    if app.running.is_none()
                        && let Some(p) = app.userland_progress().into_iter().nth(app.package_selected)
                    {
                        app.run_package(&p.id, false);
                    }
                }
                KeyCode::Char('f') => {
                    if app.running.is_none()
                        && let Some(p) = app.userland_progress().into_iter().nth(app.package_selected)
                    {
                        app.run_package(&p.id, true);
                    }
                }
                _ => {}
            },
        },
        Screen::Settings { selected } => match code {
            KeyCode::Esc | KeyCode::Char('s') => app.screen = Screen::Dashboard,
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected + 1 < settings_rows().len() {
                    *selected += 1;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => match settings_rows()[*selected] {
                SettingsRow::Networking => {
                    // Best-effort: if the config file can't be written (e.g.
                    // permissions), just leave the in-memory toggle reverted
                    // rather than surfacing a save error into a stage's log.
                    if app.toggle_networking().is_err() {
                        app.cfg.networking = !app.cfg.networking;
                    }
                }
                SettingsRow::Hostname | SettingsRow::CustomLogo => {} // press `e` to edit
                SettingsRow::Feature(i) => {
                    let key = FEATURE_PACKS[i].key;
                    let was_enabled = app.is_feature_enabled(key);
                    if app.toggle_feature(key).is_err() {
                        // Best-effort revert, same as the networking toggle above.
                        app.set_feature_enabled(key, was_enabled);
                    }
                }
            },
            KeyCode::Char('e') => match settings_rows()[*selected] {
                SettingsRow::Hostname => {
                    app.screen = Screen::EditHostname { typed: app.cfg.image.hostname.clone() };
                }
                SettingsRow::CustomLogo => app.open_file_picker(),
                _ => {}
            },
            _ => {}
        },
        Screen::EditHostname { typed } => match code {
            KeyCode::Esc => {
                app.screen = Screen::Settings {
                    selected: settings_row_index(|r| matches!(r, SettingsRow::Hostname)),
                }
            }
            KeyCode::Backspace => {
                typed.pop();
            }
            KeyCode::Char(c) => typed.push(c),
            KeyCode::Enter => {
                let typed = typed.clone();
                app.screen = Screen::Settings {
                    selected: settings_row_index(|r| matches!(r, SettingsRow::Hostname)),
                };
                let _ = app.set_hostname(&typed);
            }
            _ => {}
        },
        Screen::FilePicker { dir, entries, selected, .. } => match code {
            KeyCode::Esc => {
                app.screen = Screen::Settings {
                    selected: settings_row_index(|r| matches!(r, SettingsRow::CustomLogo)),
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected + 1 < entries.len() {
                    *selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(entry) = entries.get(*selected) {
                    if entry.name == ".." {
                        if let Some(parent) = dir.parent() {
                            let parent = parent.to_path_buf();
                            app.navigate_picker(parent);
                        }
                    } else if entry.is_dir {
                        let next = dir.join(&entry.name);
                        app.navigate_picker(next);
                    } else {
                        let path = dir.join(&entry.name);
                        app.screen = Screen::Settings {
                            selected: settings_row_index(|r| matches!(r, SettingsRow::CustomLogo)),
                        };
                        let _ = app.set_logo_file(path);
                    }
                }
            }
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
                    app.run_stage(idx, Some(&device_path), false);
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
        Screen::EditHostname { typed } => draw_edit_text(f, size, "Hostname", typed),
        Screen::FilePicker { dir, entries, selected, error } => {
            draw_file_picker(f, size, dir, entries, *selected, error.as_deref())
        }
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
            // AssembleRootfs's own is_present() just checks a marker file
            // — it has no idea the sysroot it bulk-copies from changed
            // underneath it after a userland rebuild, so that's tracked
            // separately here (App::rootfs_dirty) and shown as "!" (stale)
            // instead of "*" (present, and actually current).
            let dirty = *stage == StageKind::AssembleRootfs && app.rootfs_dirty;
            let present = if !stage.can_clean() {
                "  "
            } else if dirty {
                "! "
            } else if stage.is_present(&app.cfg, app.reg.as_ref()) {
                "* "
            } else {
                "  "
            };
            // BuildUserland is one long-running subprocess building ~25
            // packages one at a time — a bare "running" icon doesn't say
            // whether it's 1 package in or 24, so it gets a live count
            // from the same is_built() checks the log pane below uses.
            let label = if *stage == StageKind::BuildUserland {
                let progress = app.userland_progress();
                let done = progress.iter().filter(|p| p.done).count();
                format!("{} ({done}/{})", stage.label(), progress.len())
            } else if dirty {
                format!("{} (stale, rebuilds on enter)", stage.label())
            } else {
                stage.label().to_string()
            };
            let mut style = Style::default().fg(color);
            if i == app.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            ListItem::new(Line::from(Span::styled(format!("{icon}{present}{label}"), style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Pipeline stages (* = output present, ! = stale)"));
    f.render_widget(list, area);
}

fn draw_log_pane(f: &mut Frame, app: &App, area: Rect) {
    if STAGES[app.selected] == StageKind::BuildUserland {
        draw_userland_pane(f, app, area);
    } else {
        let title = format!("Log: {}", STAGES[app.selected].label());
        draw_raw_log(f, app, area, &title);
    }
}

/// BuildUserland's own pane: a package-by-package checklist on top (real
/// build order, live `is_built()` status — see `App::userland_progress`),
/// the same raw subprocess log below it so compiler errors/warnings for
/// whichever package is currently building are still visible.
fn draw_userland_pane(f: &mut Frame, app: &App, area: Rect) {
    let progress = app.userland_progress();
    let running = app.running == Some(app.selected);
    let focused = app.focus == Focus::PackageList;

    let needed = progress.len() as u16 + 2;
    let list_height = needed.min((area.height / 2).max(3));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(list_height), Constraint::Min(0)])
        .split(area);

    let done_count = progress.iter().filter(|p| p.done).count();
    // A single-package run (run_package) marks exactly that row current,
    // wherever it sits in the list; a whole-stage run always processes
    // packages in this same order, so the first not-done one is current.
    let mut whole_stage_current_marked = false;
    let items: Vec<ListItem> = progress
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_current = running
                && if let Some(running_id) = &app.running_package {
                    &p.id == running_id
                } else if !p.done && !whole_stage_current_marked {
                    whole_stage_current_marked = true;
                    true
                } else {
                    false
                };
            let (icon, color) = if p.done {
                ("OK ", Color::Green)
            } else if is_current {
                (".. ", Color::Yellow)
            } else {
                ("   ", Color::Gray)
            };
            let mut style = Style::default().fg(color);
            if focused && i == app.package_selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            ListItem::new(Line::from(Span::styled(format!("{icon}{}", p.id), style)))
        })
        .collect();
    let title = format!("Packages ({done_count}/{})", progress.len());
    let border_color = if focused { Color::Yellow } else { Color::Reset };
    let list = List::new(items).block(
        Block::default().borders(Borders::ALL).border_style(Style::default().fg(border_color)).title(title),
    );
    f.render_widget(list, chunks[0]);

    draw_raw_log(f, app, chunks[1], "Build output");
}

fn draw_raw_log(f: &mut Frame, app: &App, area: Rect, title: &str) {
    let state = &app.stages[app.selected];
    let height = area.height.saturating_sub(2) as usize;
    let start = state.log.len().saturating_sub(height);
    let lines: Vec<Line> = state.log.iter().skip(start).map(|l| Line::from(l.as_str())).collect();

    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title.to_string()));
    f.render_widget(paragraph, area);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let text = if app.running.is_some() {
        "running... (q to quit once idle)".to_string()
    } else if app.focus == Focus::PackageList {
        "up/down: select package  enter: build  f: force-rebuild  left: back  q/esc: quit".to_string()
    } else if STAGES[app.selected] == StageKind::BuildUserland {
        "up/down: select  right: packages  enter: run  f: force-run  c: clean  s: settings  q/esc: quit"
            .to_string()
    } else {
        "up/down: select  enter: run  f: force-run  c: clean  s: settings  q/esc: quit".to_string()
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
    let items: Vec<ListItem> = settings_rows()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let (mark, label, desc) = match row {
                SettingsRow::Networking => (
                    checkbox(app.cfg.networking),
                    "Networking".to_string(),
                    "busybox udhcpc/ifconfig/route/ping, DHCP at boot, QEMU NIC".to_string(),
                ),
                SettingsRow::Hostname => (
                    "   ",
                    "Hostname".to_string(),
                    format!("{} (e to change)", app.cfg.image.hostname),
                ),
                SettingsRow::CustomLogo => (
                    "   ",
                    "  Custom logo file".to_string(),
                    app.cfg
                        .packages
                        .get("kernel")
                        .and_then(|t| t.get("logo_file"))
                        .and_then(|v| v.as_str())
                        .map(|p| format!("{p} (e to change)"))
                        .unwrap_or_else(|| "stock penguin (e to pick a file)".to_string()),
                ),
                SettingsRow::Feature(idx) => {
                    let pack = &FEATURE_PACKS[*idx];
                    (checkbox(app.is_feature_enabled(pack.key)), pack.label.to_string(), pack.description.to_string())
                }
            };
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
            .title("Settings (enter/space: toggle, e: edit, esc: close)"),
    );
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(list, popup);
}

fn checkbox(on: bool) -> &'static str {
    if on { "[x]" } else { "[ ]" }
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

fn draw_edit_text(f: &mut Frame, area: Rect, label: &str, typed: &str) {
    let popup = centered_rect(60, 20, area);
    let text = vec![
        Line::from(format!("{label}: {typed}")),
        Line::from(""),
        Line::from("enter: save  esc: cancel"),
    ];
    let paragraph = Paragraph::new(text).block(Block::default().borders(Borders::ALL).title("Edit"));
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(paragraph, popup);
}

fn draw_file_picker(
    f: &mut Frame,
    area: Rect,
    dir: &std::path::Path,
    entries: &[app::PickerEntry],
    selected: usize,
    error: Option<&str>,
) {
    let popup = centered_rect(70, 60, area);
    let items: Vec<ListItem> = if let Some(error) = error {
        vec![ListItem::new(error.to_string())]
    } else if entries.is_empty() {
        vec![ListItem::new("(empty directory)")]
    } else {
        entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let label = if entry.is_dir { format!("{}/", entry.name) } else { entry.name.clone() };
                let style = if i == selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect()
    };
    let title = format!("Pick a logo file: {} (enter: open/pick, esc: cancel)", dir.display());
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(list, popup);
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
