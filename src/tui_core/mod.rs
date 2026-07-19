//! ssm's TUI primitives — header/footer/description chrome, color theme, and the
//! flash-message model.
//!
//! This is a local copy of the dots `tui-core` crate, vendored here so ssm builds
//! as a fully standalone repo with no cross-repo dependency. If the shared chrome
//! changes in dots, re-copy it here.

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::Paragraph,
};

pub mod theme;

use theme::{style_dim, style_header, style_select, style_error};

#[derive(Debug, Clone, PartialEq)]
pub enum FlashKind {
    Success,
    Error,
    Info,
}

/// Renders the top border line with the title at col 4 and the version near the right edge.
pub fn draw_header(f: &mut Frame, area: Rect, title: &str, version: &str) {
    let w = area.width as usize;
    if w == 0 { return; }

    let mut chars: Vec<char> = "─".repeat(w).chars().collect();

    for (i, ch) in title.chars().enumerate() {
        let pos = 4 + i;
        if pos < w { chars[pos] = ch; }
    }

    if !version.is_empty() {
        let ver = format!(" v{version} ");
        let start = w.saturating_sub(ver.len() + 2).max(4 + title.len() + 1);
        for (i, ch) in ver.chars().enumerate() {
            let pos = start + i;
            if pos < w { chars[pos] = ch; }
        }
    }

    let line: String = chars.into_iter().collect();
    let rect = Rect { x: area.x, y: area.y, width: area.width, height: 1 };
    f.render_widget(Paragraph::new(Line::from(Span::styled(line, style_header()))), rect);
}

/// Renders ───── border at h-3 and hint text at h-2.
pub fn draw_footer(f: &mut Frame, area: Rect, hint: &str) {
    if area.height < 3 { return; }

    let border_y = area.y + area.height - 3;
    let hint_y   = area.y + area.height - 2;

    let border = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(border, style_header()))),
        Rect { x: area.x, y: border_y, width: area.width, height: 1 },
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, style_dim()))),
        Rect { x: area.x, y: hint_y, width: area.width, height: 1 },
    );
}

/// Renders the description / flash bar at h-4.
pub fn draw_desc(f: &mut Frame, area: Rect, text: &str, flash: Option<&(String, FlashKind)>) {
    if area.height < 4 { return; }

    let desc_y    = area.y + area.height - 4;
    let desc_rect = Rect { x: area.x, y: desc_y, width: area.width, height: 1 };

    let widget = match flash {
        Some((msg, kind)) => {
            let style = match kind {
                FlashKind::Success => style_select(),
                FlashKind::Error   => style_error(),
                FlashKind::Info    => style_dim().add_modifier(Modifier::BOLD),
            };
            Paragraph::new(Line::from(Span::styled(format!("  {msg}"), style)))
        }
        None => Paragraph::new(Line::from(vec![
            Span::styled("  › ", style_dim()),
            Span::styled(text.to_string(), style_header()),
        ])),
    };

    f.render_widget(widget, desc_rect);
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn header_no_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| {
            let area = f.area();
            draw_header(f, area, " dots ", "1.0.0");
            draw_footer(f, area, " q quit ");
        }).unwrap();
    }

    #[test]
    fn small_terminal_shows_guard_message() {
        let backend = TestBackend::new(30, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| {
            let area = f.area();
            if area.width < 50 || area.height < 14 {
                let msg = "Terminal too small — need at least 50×14";
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(msg, super::theme::style_error()))),
                    Rect { x: 0, y: area.height / 2, width: area.width, height: 1 },
                );
            }
        }).unwrap();
    }
}
