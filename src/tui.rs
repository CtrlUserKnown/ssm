use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
};
use ratatui::{
    Frame,
    backend::CrosstermBackend,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Terminal,
};

use crate::tui_core::{draw_desc, draw_footer, draw_header, FlashKind};
use crate::tui_core::theme;
use crate::tui_core::theme::{style_dim, style_error, style_header, style_select};
use super::config;
use super::connect::{do_connect, ConnectConfig};
use super::storage::{kr_delete, load_sessions, save_sessions, sessions_mtime, Session};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── screens ───────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum SsmScreen {
    List,
    Form,
    Search,
    Help,
    ConfirmDelete,
}

// ── which-key menu ──────────────────────────────────────────────────────────────
//
// A transient leader-key popup: press a prefix key on the list, a small box appears
// in the bottom-right listing the next keys, press one to act (or esc to dismiss).

#[derive(Debug, Clone, Copy, PartialEq)]
enum WhichKey {
    /// Top-level actions leader (opened with space): delete, yank, settings, help.
    Actions,
    /// Settings submenu, reachable from Actions via `s`.
    Settings,
    /// Theme picker, reachable from Settings via `t`.
    Theme,
}

/// One selectable line in a which-key popup.
struct MenuRow {
    key:  &'static str,
    desc: String,
}

impl WhichKey {
    fn title(self) -> &'static str {
        match self {
            WhichKey::Actions  => " actions ",
            WhichKey::Settings => " settings ",
            WhichKey::Theme    => " theme ",
        }
    }

    fn rows(self, app: &SsmApp) -> Vec<MenuRow> {
        match self {
            WhichKey::Actions => vec![
                MenuRow { key: "d", desc: "delete".to_string() },
                MenuRow { key: "y", desc: "yank host".to_string() },
                MenuRow { key: "s", desc: "settings \u{203a}".to_string() },
                MenuRow { key: "?", desc: "help".to_string() },
            ],
            WhichKey::Settings => vec![
                MenuRow { key: "h", desc: format!("herdr   {}", if app.cfg.use_herdr { "on" } else { "off" }) },
                MenuRow { key: "t", desc: "theme \u{203a}".to_string() },
            ],
            // One row per theme; the active one is marked with a dot.
            WhichKey::Theme => theme::THEMES.iter().map(|t| {
                let mark = if t.name == app.theme { " \u{25cf}" } else { "" };
                MenuRow { key: t.key, desc: format!("{}{}", t.name, mark) }
            }).collect(),
        }
    }
}

// ── form ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum FormMode { Add, Edit }

const FIELD_LABELS: &[&str] = &["Name", "Host", "User", "Port", "Password"];
const FIELD_COUNT: usize = FIELD_LABELS.len();

struct FormState {
    fields: [String; FIELD_COUNT],
    cursor: usize,
    mode:   FormMode,
    edit_idx: Option<usize>,
    /// When true the selected field has keyboard focus and captures text input.
    /// When false the form is in navigation mode (j/k move between fields).
    focused: bool,
}

impl FormState {
    fn new_add() -> Self {
        Self {
            fields:   [String::new(), String::new(), String::new(), "22".to_string(), String::new()],
            cursor:   0,
            mode:     FormMode::Add,
            edit_idx: None,
            focused:  false,
        }
    }

    fn from_session(s: &Session, idx: usize) -> Self {
        Self {
            fields: [
                s.name.clone(),
                s.host.clone(),
                s.user.clone(),
                s.port.to_string(),
                s.password.clone(),
            ],
            cursor:   0,
            mode:     FormMode::Edit,
            edit_idx: Some(idx),
            focused:  false,
        }
    }

    fn to_session(&self) -> Option<Session> {
        let (name, host, user, port_str, password) = (
            self.fields[0].trim().to_string(),
            self.fields[1].trim().to_string(),
            self.fields[2].trim().to_string(),
            self.fields[3].trim(),
            self.fields[4].clone(),
        );
        if name.is_empty() || host.is_empty() || user.is_empty() { return None; }
        let port = port_str.parse::<u16>().ok()?;
        Some(Session { name, host, user, port, password })
    }

    fn validate(&self) -> Option<String> {
        if self.fields[0].trim().is_empty() { return Some("Name is required".to_string()); }
        if self.fields[1].trim().is_empty() { return Some("Host is required".to_string()); }
        if self.fields[2].trim().is_empty() { return Some("User is required".to_string()); }
        if self.fields[3].trim().parse::<u16>().is_err() {
            return Some("Port must be 1-65535".to_string());
        }
        None
    }
}

// ── main app struct ───────────────────────────────────────────────────────────

pub struct SsmApp {
    sessions:       Vec<Session>,
    cfg:            ConnectConfig,
    idx:            usize,
    screen:         SsmScreen,
    menu:           Option<WhichKey>,
    theme:          String,
    flash:          Option<(String, FlashKind)>,
    form:           FormState,
    search_query:   String,
    filter_active:  bool,
    visible:        Vec<usize>,
    count_buf:      String,
    pending_g:      bool,
    last_mtime:     f64,
    should_quit:    bool,
    connect_target: Option<Session>,
}

impl SsmApp {
    fn new(cfg: config::SsmConfig) -> Self {
        let sessions   = load_sessions().unwrap_or_default();
        let last_mtime = sessions_mtime();
        let count      = sessions.len();
        Self {
            sessions,
            cfg:            ConnectConfig { use_herdr: cfg.use_herdr },
            idx:            0,
            screen:         SsmScreen::List,
            menu:           None,
            theme:          cfg.theme,
            flash:          None,
            form:           FormState::new_add(),
            search_query:   String::new(),
            filter_active:  false,
            visible:        (0..count).collect(),
            count_buf:      String::new(),
            pending_g:      false,
            last_mtime,
            should_quit:    false,
            connect_target: None,
        }
    }

    fn reload_if_changed(&mut self) {
        let mtime = sessions_mtime();
        if mtime > self.last_mtime {
            if let Ok(s) = load_sessions() {
                self.sessions   = s;
                self.last_mtime = mtime;
                self.rebuild_visible();
                self.clamp_idx();
            }
        }
    }

    fn rebuild_visible(&mut self) {
        if self.filter_active && !self.search_query.is_empty() {
            let q = self.search_query.to_lowercase();
            self.visible = self.sessions.iter().enumerate()
                .filter(|(_, s)| {
                    s.name.to_lowercase().contains(&q)
                        || s.host.to_lowercase().contains(&q)
                        || s.user.to_lowercase().contains(&q)
                })
                .map(|(i, _)| i)
                .collect();
        } else {
            self.visible = (0..self.sessions.len()).collect();
        }
    }

    fn active_len(&self) -> usize { self.visible.len() }

    fn active_session(&self, display_idx: usize) -> Option<&Session> {
        let raw = self.visible.get(display_idx)?;
        self.sessions.get(*raw)
    }

    fn clamp_idx(&mut self) {
        let n = self.active_len();
        if n == 0 { self.idx = 0; } else { self.idx = self.idx.min(n - 1); }
    }

    fn take_count(&mut self) -> usize {
        let s = self.count_buf.trim().to_string();
        self.count_buf.clear();
        s.parse::<usize>().unwrap_or(1)
    }

    fn move_up(&mut self, n: usize) {
        self.idx = self.idx.saturating_sub(n);
    }

    fn move_down(&mut self, n: usize) {
        let max = self.active_len().saturating_sub(1);
        self.idx = (self.idx + n).min(max);
    }
}

// ── public entry point ────────────────────────────────────────────────────────

pub fn run_ssm(terminal: &mut Terminal<CrosstermBackend<Stdout>>, cfg: config::SsmConfig) -> anyhow::Result<()> {
    let mut app = SsmApp::new(cfg);

    loop {
        app.reload_if_changed();

        terminal.draw(|f| render_ssm(f, &app))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                handle_ssm_key(&mut app, key);
            }
        }

        if app.should_quit { break; }

        if let Some(session) = app.connect_target.take() {
            do_connect_with_resume(terminal, &session, &app.cfg)?;
        }
    }
    Ok(())
}

fn do_connect_with_resume(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    session:  &Session,
    cfg:      &ConnectConfig,
) -> anyhow::Result<()> {
    // Temporarily leave TUI
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    println!("Connecting to {}@{}:{} …\n", session.user, session.host, session.port);
    if let Err(e) = do_connect(session, cfg) {
        eprintln!("\nConnection error: {e}");
    }

    println!("\nPress Enter to return to SSM…");
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();

    // Re-enter TUI
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

// ── rendering ─────────────────────────────────────────────────────────────────

fn render_ssm(f: &mut Frame, app: &SsmApp) {
    let area = f.area();
    if area.width < 50 || area.height < 10 {
        let y = area.height / 2;
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Terminal too small — need at least 50×10", style_error(),
            ))),
            Rect { x: area.x, y: area.y + y, width: area.width, height: 1 },
        );
        return;
    }

    match &app.screen {
        SsmScreen::List           => render_list(f, area, app),
        SsmScreen::Form           => render_form(f, area, app),
        SsmScreen::Search         => render_search(f, area, app),
        SsmScreen::Help           => render_help(f, area, app),
        SsmScreen::ConfirmDelete  => render_list_with_confirm(f, area, app),
    }

    // The which-key popup overlays whatever screen is behind it.
    if let Some(menu) = app.menu {
        render_which_key(f, area, app, menu);
    }
}

/// A which-key leader popup, drawn as a small bordered box in the bottom-right,
/// floating just above the desc/footer chrome.
fn render_which_key(f: &mut Frame, area: Rect, app: &SsmApp, menu: WhichKey) {
    let title = menu.title();
    let rows  = menu.rows(app);
    let esc_hint = "esc  close";

    // Widest line drives the inner width (rows, the esc hint, and the title).
    let content_w = rows.iter()
        .map(|r| r.key.len() + 2 + r.desc.len())
        .chain(std::iter::once(esc_hint.len()))
        .chain(std::iter::once(title.len()))
        .max()
        .unwrap_or(12);

    // +2 borders, +2 inner horizontal padding.
    let box_w = (content_w as u16 + 4).min(area.width.saturating_sub(2));
    // rows + the esc line + top/bottom border.
    let box_h = rows.len() as u16 + 1 + 2;

    let x = area.x + area.width.saturating_sub(box_w + 1);
    let y = area.y + area.height.saturating_sub(box_h + 4);
    let rect = Rect { x, y, width: box_w, height: box_h };

    f.render_widget(Clear, rect);
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(style_header())
            .title(Span::styled(title, style_header())),
        rect,
    );

    let inner_x = rect.x + 2;
    let inner_w = rect.width.saturating_sub(3);
    for (i, r) in rows.iter().enumerate() {
        let ly = rect.y + 1 + i as u16;
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{:<2}", r.key), style_select().add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {}", r.desc), ratatui::style::Style::default()),
            ])),
            Rect { x: inner_x, y: ly, width: inner_w, height: 1 },
        );
    }

    let esc_y = rect.y + 1 + rows.len() as u16;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(esc_hint, style_dim()))),
        Rect { x: inner_x, y: esc_y, width: inner_w, height: 1 },
    );
}

fn render_list(f: &mut Frame, area: Rect, app: &SsmApp) {
    draw_header(f, area, " ssm ", VERSION);
    render_session_rows(f, area, app, None);
    draw_desc(f, area, "", app.flash.as_ref());
    draw_footer(f, area, " j/k move  enter connect  a add  e edit  / search  space menu  q quit ");
}

fn render_list_with_confirm(f: &mut Frame, area: Rect, app: &SsmApp) {
    draw_header(f, area, " ssm ", VERSION);
    render_session_rows(f, area, app, None);
    if let Some(s) = app.active_session(app.idx) {
        let msg = format!("Delete '{}'? y/n", s.name);
        draw_desc(f, area, &msg, None);
    }
    draw_footer(f, area, " y confirm  n cancel ");
}

fn render_session_rows(f: &mut Frame, area: Rect, app: &SsmApp, highlight: Option<&str>) {
    let header_y = area.y + 2;
    if area.height < 5 { return; }

    // Column header
    let hdr = format!("  {:<20} {:<22} {:<6}", "NAME", "HOST", "PORT");
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(hdr, style_dim()))),
        Rect { x: area.x + 2, y: header_y, width: area.width.saturating_sub(4), height: 1 },
    );

    let list_y    = header_y + 1;
    let list_h    = area.height.saturating_sub(7) as usize;
    let start_row = if app.idx >= list_h { app.idx - list_h + 1 } else { 0 };

    for (row, &raw_idx) in app.visible.iter().enumerate().skip(start_row).take(list_h) {
        let y = list_y + (row - start_row) as u16;
        if y + 4 >= area.y + area.height { break; }

        let Some(s) = app.sessions.get(raw_idx) else { continue };
        let is_sel  = row == app.idx;
        let cursor  = if is_sel { "▶ " } else { "  " };

        let host_col = format!("{}@{}", s.user, s.host);
        let name_col = if let Some(q) = highlight {
            if s.name.to_lowercase().contains(&q.to_lowercase()) {
                format!("{:<20}", s.name)
            } else {
                format!("{:<20}", &s.name[..s.name.len().min(20)])
            }
        } else {
            format!("{:<20}", &s.name[..s.name.len().min(20)])
        };

        let line = format!("{cursor}{name_col} {host_col:<22} {}", s.port);
        let style = if is_sel {
            style_select().add_modifier(Modifier::BOLD)
        } else {
            ratatui::style::Style::default()
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(line, style))),
            Rect { x: area.x + 2, y, width: area.width.saturating_sub(4), height: 1 },
        );
    }

    if app.active_len() == 0 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled("No sessions. Press 'a' to add one.", style_dim()))),
            Rect { x: area.x + 4, y: list_y, width: area.width.saturating_sub(8), height: 1 },
        );
    }
}

fn render_form(f: &mut Frame, area: Rect, app: &SsmApp) {
    let title = match app.form.mode {
        FormMode::Add  => " add session ",
        FormMode::Edit => " edit session ",
    };
    draw_header(f, area, title, VERSION);

    for (i, label) in FIELD_LABELS.iter().enumerate() {
        let y = area.y + 2 + i as u16 * 2;
        if y + 4 >= area.y + area.height { break; }

        let is_sel  = i == app.form.cursor;
        let editing = is_sel && app.form.focused;
        let value   = &app.form.fields[i];
        let display = if i == 4 && !value.is_empty() { "*".repeat(value.len()) } else { value.clone() };

        let label_style = if is_sel { style_select().add_modifier(Modifier::BOLD) } else { style_dim() };
        let val_style   = if editing {
            style_select().add_modifier(Modifier::BOLD)
        } else if is_sel {
            ratatui::style::Style::default().add_modifier(Modifier::BOLD)
        } else {
            ratatui::style::Style::default()
        };
        let cursor = if is_sel { "▶ " } else { "  " };
        // The block caret only shows on the field that currently has edit focus.
        let caret  = if editing { "█" } else { "" };

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(cursor, label_style),
                Span::styled(format!("{:<12}", label), label_style),
                Span::styled(format!("{display}{caret}"), val_style),
            ])),
            Rect { x: area.x + 2, y, width: area.width.saturating_sub(4), height: 1 },
        );
    }

    if app.form.focused {
        draw_desc(f, area, "editing — type to edit  tab next field  enter/esc leave field", app.flash.as_ref());
        draw_footer(f, area, " type to edit  tab/shift-tab next/prev  enter/esc leave field ");
    } else {
        draw_desc(f, area, "j/k · ↑/↓ navigate  enter edit field  S save  esc cancel", app.flash.as_ref());
        draw_footer(f, area, " j/k move  enter edit field  S save  esc cancel ");
    }
}

fn render_search(f: &mut Frame, area: Rect, app: &SsmApp) {
    draw_header(f, area, " search sessions ", VERSION);
    render_session_rows(f, area, app, Some(&app.search_query));

    let prompt_y = area.y + area.height.saturating_sub(4);
    let q_display = format!("/ {}_", app.search_query);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(q_display, style_header()))),
        Rect { x: area.x + 2, y: prompt_y, width: area.width.saturating_sub(4), height: 1 },
    );

    draw_footer(f, area, " type to filter  enter apply  esc cancel ");
}

fn render_help(f: &mut Frame, area: Rect, app: &SsmApp) {
    draw_header(f, area, " ssm help ", VERSION);
    let lines: &[(&str, &str)] = &[
        ("Navigation", ""),
        ("  j / k",    "move up/down"),
        ("  gg / G",   "go to first/last"),
        ("  C-d / C-u","half page down/up"),
        ("  C-f / C-b","full page down/up"),
        ("", ""),
        ("Actions", ""),
        ("  enter",    "connect to session"),
        ("  a",        "add new session"),
        ("  e",        "edit selected session"),
        ("  D",        "delete session (prompts)"),
        ("  d d",      "delete session (prompts)"),
        ("  y",        "yank (copy) host to clipboard"),
        ("  /",        "search/filter sessions"),
        ("  s",        "settings menu (h herdr, t theme)"),
        ("  u",        "reload sessions from disk"),
        ("  ?",        "this help screen"),
        ("  q",        "quit SSM"),
        ("", ""),
        ("More info", ""),
        ("  github.com/ctrl-felix/dots/wiki/ssm", ""),
    ];

    for (i, (key, val)) in lines.iter().enumerate() {
        let y = area.y + 2 + i as u16;
        if y + 4 >= area.y + area.height { break; }
        if key.is_empty() { continue; }
        let style = if val.is_empty() {
            style_header().add_modifier(Modifier::BOLD)
        } else {
            ratatui::style::Style::default()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{:<26}", key), style),
                Span::styled(*val, style_dim()),
            ])),
            Rect { x: area.x + 2, y, width: area.width.saturating_sub(4), height: 1 },
        );
    }

    draw_desc(f, area, "", app.flash.as_ref());
    draw_footer(f, area, " q back ");
}

// ── key handling ──────────────────────────────────────────────────────────────

fn handle_ssm_key(app: &mut SsmApp, key: KeyEvent) {
    // A which-key popup captures all input until it's dismissed or acted on.
    if let Some(menu) = app.menu {
        handle_which_key(app, menu, key);
        return;
    }

    match &app.screen {
        SsmScreen::List          => handle_list_key(app, key),
        SsmScreen::Form          => handle_form_key(app, key),
        SsmScreen::Search        => handle_search_key(app, key),
        SsmScreen::Help          => handle_help_key(app, key),
        SsmScreen::ConfirmDelete => handle_confirm_key(app, key),
    }
}

fn handle_which_key(app: &mut SsmApp, menu: WhichKey, key: KeyEvent) {
    match menu {
        WhichKey::Actions => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => { app.menu = None; }
            KeyCode::Char('d') => { app.menu = None; request_delete(app); }
            KeyCode::Char('y') => { app.menu = None; yank_selected(app); }
            // Descend into the settings submenu (stay in the popup).
            KeyCode::Char('s') => { app.menu = Some(WhichKey::Settings); }
            KeyCode::Char('?') => { app.menu = None; app.screen = SsmScreen::Help; }
            _ => {}
        },
        WhichKey::Settings => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => { app.menu = None; }
            KeyCode::Char('h') => {
                app.cfg.use_herdr = !app.cfg.use_herdr;
                let state = if app.cfg.use_herdr { "enabled" } else { "disabled" };
                match persist_config(app) {
                    Ok(())  => app.flash = Some((format!("✓ herdr {state}"), FlashKind::Success)),
                    Err(e)  => app.flash = Some((format!("✗ Save failed: {e}"), FlashKind::Error)),
                }
                app.menu = None;
            }
            // Descend into the theme picker (stay in the popup).
            KeyCode::Char('t') => { app.menu = Some(WhichKey::Theme); }
            _ => {}
        },
        WhichKey::Theme => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => { app.menu = None; }
            KeyCode::Char(c) => {
                if let Some(t) = theme::THEMES.iter().find(|t| t.key.starts_with(c)) {
                    app.theme = t.name.to_string();
                    theme::set_theme(&app.theme);
                    match persist_config(app) {
                        Ok(())  => app.flash = Some((format!("✓ theme: {}", t.name), FlashKind::Success)),
                        Err(e)  => app.flash = Some((format!("✗ Save failed: {e}"), FlashKind::Error)),
                    }
                    // Stay open so the user can eyeball other themes live.
                }
            }
            _ => {}
        },
    }
}

/// Persist the current preferences (herdr + theme) to disk.
fn persist_config(app: &SsmApp) -> anyhow::Result<()> {
    config::save(&config::SsmConfig {
        use_herdr: app.cfg.use_herdr,
        theme:     app.theme.clone(),
    })
}

fn handle_list_key(app: &mut SsmApp, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let half_page  = 10usize;
    let full_page  = 20usize;

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => { app.should_quit = true; }
        KeyCode::Char('?') | KeyCode::Char('h') => {
            app.screen = SsmScreen::Help;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let n = app.take_count();
            app.move_down(n);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let n = app.take_count();
            app.move_up(n);
        }
        KeyCode::Char('g') => {
            if app.pending_g {
                app.idx      = 0;
                app.pending_g = false;
                app.count_buf.clear();
            } else {
                app.pending_g = true;
            }
        }
        KeyCode::Char('G') => {
            app.idx        = app.active_len().saturating_sub(1);
            app.pending_g  = false;
            app.count_buf.clear();
        }
        KeyCode::Char('d') if ctrl => { app.move_down(half_page); }
        KeyCode::Char('u') if ctrl => { app.move_up(half_page); }
        KeyCode::Char('f') if ctrl => { app.move_down(full_page); }
        KeyCode::Char('b') if ctrl => { app.move_up(full_page); }
        KeyCode::Char('u') => {
            // reload
            if let Ok(s) = load_sessions() {
                app.sessions   = s;
                app.last_mtime = sessions_mtime();
                app.rebuild_visible();
                app.clamp_idx();
                app.flash = Some(("✓ Reloaded".to_string(), FlashKind::Success));
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            app.pending_g = false;
            app.count_buf.push(c);
        }
        KeyCode::Enter => {
            if let Some(s) = app.active_session(app.idx).cloned() {
                app.connect_target = Some(s);
            }
        }
        KeyCode::Char('a') => {
            app.form   = FormState::new_add();
            app.screen = SsmScreen::Form;
            app.flash  = None;
        }
        KeyCode::Char('e') => {
            if let Some(s) = app.active_session(app.idx).cloned() {
                let raw = app.visible[app.idx];
                app.form   = FormState::from_session(&s, raw);
                app.screen = SsmScreen::Form;
                app.flash  = None;
            }
        }
        KeyCode::Char(' ') => {
            app.menu  = Some(WhichKey::Actions);
            app.flash = None;
        }
        KeyCode::Char('D') => request_delete(app),
        KeyCode::Char('y') => yank_selected(app),
        KeyCode::Char('/') => {
            app.screen       = SsmScreen::Search;
            app.search_query = String::new();
            app.flash        = None;
        }
        KeyCode::Char('s') => {
            app.menu  = Some(WhichKey::Settings);
            app.flash = None;
        }
        _ => { app.pending_g = false; }
    }
}

fn handle_form_key(app: &mut SsmApp, key: KeyEvent) {
    if app.form.focused {
        handle_form_edit_key(app, key);
    } else {
        handle_form_nav_key(app, key);
    }
}

/// Navigation mode: no field has focus, so j/k (and arrows/Tab) move the cursor
/// between fields. Enter drops into the selected field; Esc leaves the form.
fn handle_form_nav_key(app: &mut SsmApp, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.screen = SsmScreen::List;
            app.flash  = None;
        }
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
            app.form.cursor = (app.form.cursor + 1) % FIELD_COUNT;
        }
        KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
            app.form.cursor = (app.form.cursor + FIELD_COUNT - 1) % FIELD_COUNT;
        }
        KeyCode::Enter | KeyCode::Char('i') => {
            app.form.focused = true;
        }
        // Save the whole form from anywhere in nav mode (no need to reach the last field).
        KeyCode::Char('S') => save_form(app),
        _ => {}
    }
}

/// Edit mode: the selected field captures text. Tab / Shift-Tab hop between fields
/// while still editing; Enter or Esc leaves the field back to navigation mode, where
/// `S` saves the whole form.
fn handle_form_edit_key(app: &mut SsmApp, key: KeyEvent) {
    match key.code {
        // Both leave the field and return to navigation mode; save is `S` from there.
        KeyCode::Enter | KeyCode::Esc => {
            app.form.focused = false;
        }
        // Tab / Shift-Tab move to the next / previous field without leaving edit mode.
        KeyCode::Tab => {
            app.form.cursor = (app.form.cursor + 1) % FIELD_COUNT;
        }
        KeyCode::BackTab => {
            app.form.cursor = (app.form.cursor + FIELD_COUNT - 1) % FIELD_COUNT;
        }
        KeyCode::Backspace => {
            app.form.fields[app.form.cursor].pop();
        }
        KeyCode::Char(c) => {
            app.form.fields[app.form.cursor].push(c);
        }
        _ => {}
    }
}

/// Validate the form and persist it. On success returns to the list; on any
/// failure sets a flash message and leaves the user in the form.
fn save_form(app: &mut SsmApp) {
    if let Some(err) = app.form.validate() {
        app.flash = Some((err, FlashKind::Error));
        return;
    }
    let Some(mut session) = app.form.to_session() else { return };
    match app.form.mode {
        FormMode::Add => {
            if app.sessions.iter().any(|s| s.name == session.name) {
                app.flash = Some((
                    format!("Name '{}' already exists", session.name),
                    FlashKind::Error,
                ));
                return;
            }
            app.sessions.push(session);
        }
        FormMode::Edit => {
            if let Some(raw_idx) = app.form.edit_idx {
                if let Some(s) = app.sessions.get_mut(raw_idx) {
                    // Keep password if form left it empty (don't wipe stored password)
                    if session.password.is_empty() {
                        session.password = s.password.clone();
                    }
                    *s = session;
                }
            }
        }
    }
    if let Err(e) = save_sessions(&app.sessions) {
        app.flash = Some((format!("✗ Save failed: {e}"), FlashKind::Error));
    } else {
        app.last_mtime = sessions_mtime();
        app.rebuild_visible();
        app.clamp_idx();
        app.screen = SsmScreen::List;
        app.flash  = Some(("✓ Saved".to_string(), FlashKind::Success));
    }
}

fn handle_search_key(app: &mut SsmApp, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            // Cancel — restore full list
            app.filter_active  = false;
            app.search_query   = String::new();
            app.rebuild_visible();
            app.clamp_idx();
            app.screen = SsmScreen::List;
        }
        KeyCode::Enter => {
            // Apply filter and return to list
            app.filter_active = !app.search_query.is_empty();
            app.rebuild_visible();
            app.idx    = 0;
            app.screen = SsmScreen::List;
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.rebuild_visible();
            app.clamp_idx();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.rebuild_visible();
            app.clamp_idx();
        }
        _ => {}
    }
}

fn handle_help_key(app: &mut SsmApp, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => { app.screen = SsmScreen::List; }
        _ => {}
    }
}

fn handle_confirm_key(app: &mut SsmApp, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') => {
            if let Some(&raw_idx) = app.visible.get(app.idx) {
                let name = app.sessions[raw_idx].name.clone();
                kr_delete(&name);
                app.sessions.remove(raw_idx);
                if let Err(e) = save_sessions(&app.sessions) {
                    app.flash = Some((format!("✗ Save failed: {e}"), FlashKind::Error));
                } else {
                    app.last_mtime = sessions_mtime();
                    app.flash      = Some((format!("✓ Deleted '{name}'"), FlashKind::Success));
                }
                app.rebuild_visible();
                app.clamp_idx();
            }
            app.screen = SsmScreen::List;
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            app.screen = SsmScreen::List;
            app.flash  = None;
        }
        _ => {}
    }
}

// ── list actions ──────────────────────────────────────────────────────────────

/// Open the delete confirmation for the selected session (no-op when the list is empty).
fn request_delete(app: &mut SsmApp) {
    if !app.sessions.is_empty() {
        app.screen = SsmScreen::ConfirmDelete;
    }
}

/// Copy the selected session's `user@host` to the clipboard, reporting via flash.
fn yank_selected(app: &mut SsmApp) {
    if let Some(s) = app.active_session(app.idx) {
        let text = format!("{}@{}", s.user, s.host);
        match yank(&text) {
            Ok(())  => app.flash = Some((format!("✓ Copied '{text}' to clipboard"), FlashKind::Success)),
            Err(e)  => app.flash = Some((format!("✗ {e}"), FlashKind::Error)),
        }
    }
}

// ── clipboard ─────────────────────────────────────────────────────────────────

fn yank(text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let candidates: &[(&str, &[&str])] = &[
        ("pbcopy",  &[]),
        ("xclip",   &["-selection", "clipboard"]),
        ("xsel",    &["--clipboard", "--input"]),
    ];

    for (cmd, args) in candidates {
        let Ok(mut child) = Command::new(cmd).args(*args).stdin(Stdio::piped()).spawn() else {
            continue;
        };
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes()).ok();
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
    }
    anyhow::bail!("clipboard tool not found — install xclip or xsel")
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> SsmApp {
        SsmApp::new(config::SsmConfig { use_herdr: false, theme: "auto".to_string() })
    }

    #[test]
    fn form_rejects_empty_name() {
        let mut f = FormState::new_add();
        f.fields[1] = "host".to_string();
        f.fields[2] = "user".to_string();
        f.fields[3] = "22".to_string();
        assert!(f.validate().is_some());
    }

    #[test]
    fn form_rejects_bad_port() {
        let mut f = FormState::new_add();
        f.fields[0] = "myhost".to_string();
        f.fields[1] = "1.2.3.4".to_string();
        f.fields[2] = "alice".to_string();
        f.fields[3] = "notaport".to_string();
        assert!(f.validate().is_some());
    }

    #[test]
    fn form_accepts_valid_session() {
        let mut f = FormState::new_add();
        f.fields[0] = "prod".to_string();
        f.fields[1] = "10.0.0.1".to_string();
        f.fields[2] = "root".to_string();
        f.fields[3] = "22".to_string();
        assert!(f.validate().is_none());
        assert!(f.to_session().is_some());
    }

    #[test]
    fn search_filters_by_name() {
        let mut app = make_app();
        app.sessions = vec![
            Session { name: "prod-web".to_string(), host: "1.1.1.1".to_string(), user: "alice".to_string(), port: 22, password: String::new() },
            Session { name: "staging".to_string(),  host: "2.2.2.2".to_string(), user: "bob".to_string(),   port: 22, password: String::new() },
            Session { name: "dev-box".to_string(),  host: "3.3.3.3".to_string(), user: "carol".to_string(), port: 22, password: String::new() },
        ];
        app.search_query  = "prod".to_string();
        app.filter_active = true;
        app.rebuild_visible();

        assert_eq!(app.active_len(), 1);
        assert_eq!(app.active_session(0).unwrap().name, "prod-web");
    }

    #[test]
    fn which_key_popup_renders_settings() {
        use ratatui::{backend::TestBackend, Terminal};

        let mut app = make_app();
        app.menu = Some(WhichKey::Settings);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_ssm(f, &app)).unwrap();

        let text: String = terminal.backend().buffer().content()
            .iter().map(|c| c.symbol()).collect();
        assert!(text.contains("settings"), "popup title missing");
        assert!(text.contains("herdr"),    "herdr row missing");
        assert!(text.contains("theme"),    "theme row missing");
    }

    #[test]
    fn search_filters_by_host() {
        let mut app = make_app();
        app.sessions = vec![
            Session { name: "a".to_string(), host: "alpha.example.com".to_string(), user: "u".to_string(), port: 22, password: String::new() },
            Session { name: "b".to_string(), host: "beta.example.com".to_string(),  user: "u".to_string(), port: 22, password: String::new() },
        ];
        app.search_query  = "alpha".to_string();
        app.filter_active = true;
        app.rebuild_visible();

        assert_eq!(app.active_len(), 1);
    }
}
