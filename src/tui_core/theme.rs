//! Color themes for ssm's TUI.
//!
//! Only four *foreground* accent roles are themed — the terminal's own background
//! shows through — so a theme is fully described by these roles. The active theme
//! lives in a process-global so the `style_*()` helpers stay call-site-compatible
//! with the rest of the UI (no `&Theme` threaded through every render fn).

use ratatui::style::{Color, Modifier, Style};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A named color scheme.
pub struct Theme {
    pub name:   &'static str,
    /// Mnemonic key used to pick this theme in the picker popup.
    pub key:    &'static str,
    /// Primary accent: titles, borders, prompts.
    pub header: Color,
    /// Success / selection accent.
    pub select: Color,
    /// Errors and "off" states.
    pub error:  Color,
    /// Muted secondary text.
    pub dim:    Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color { Color::Rgb(r, g, b) }

/// All selectable themes. `auto` (index 0) is the default and follows the
/// terminal's own 16-color ANSI palette; the rest pin explicit RGB values.
pub const THEMES: &[Theme] = &[
    Theme {
        name: "auto", key: "a",
        header: Color::Cyan, select: Color::Green, error: Color::Red, dim: Color::DarkGray,
    },
    // Catppuccin-Mocha accents on a noir background — src/ghostty/themes/noir-cat.
    Theme {
        name: "noir-cat", key: "o",
        header: rgb(0xcb, 0xa6, 0xf7), select: rgb(0xa6, 0xe3, 0xa1),
        error:  rgb(0xf3, 0x8b, 0xa8), dim:    rgb(0x58, 0x5b, 0x70),
    },
    // Rosé Pine — src/ghostty/themes/knew-pines.
    Theme {
        name: "knew-pines", key: "k",
        header: rgb(0xc4, 0xa7, 0xe7), select: rgb(0x9c, 0xcf, 0xd8),
        error:  rgb(0xeb, 0x6f, 0x92), dim:    rgb(0x6e, 0x6a, 0x86),
    },
    // Catppuccin Mocha.
    Theme {
        name: "catppuccin", key: "c",
        header: rgb(0x89, 0xb4, 0xfa), select: rgb(0xa6, 0xe3, 0xa1),
        error:  rgb(0xf3, 0x8b, 0xa8), dim:    rgb(0x58, 0x5b, 0x70),
    },
    // Gruvbox (dark).
    Theme {
        name: "gruvbox", key: "g",
        header: rgb(0x83, 0xa5, 0x98), select: rgb(0xb8, 0xbb, 0x26),
        error:  rgb(0xfb, 0x49, 0x34), dim:    rgb(0x92, 0x83, 0x74),
    },
    // Nord.
    Theme {
        name: "nord", key: "n",
        header: rgb(0x88, 0xc0, 0xd0), select: rgb(0xa3, 0xbe, 0x8c),
        error:  rgb(0xbf, 0x61, 0x6a), dim:    rgb(0x4c, 0x56, 0x6a),
    },
    // Tokyo Night.
    Theme {
        name: "tokyo-night", key: "t",
        header: rgb(0x7a, 0xa2, 0xf7), select: rgb(0x9e, 0xce, 0x6a),
        error:  rgb(0xf7, 0x76, 0x8e), dim:    rgb(0x56, 0x5f, 0x89),
    },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Point the global theme at `name`. Returns `false` (and changes nothing) if the
/// name is unknown — e.g. a stale value in the config file.
pub fn set_theme(name: &str) -> bool {
    match THEMES.iter().position(|t| t.name == name) {
        Some(i) => { CURRENT.store(i, Ordering::Relaxed); true }
        None    => false,
    }
}

/// The active theme.
pub fn current() -> &'static Theme {
    &THEMES[CURRENT.load(Ordering::Relaxed).min(THEMES.len() - 1)]
}

pub fn style_header() -> Style { Style::default().fg(current().header).add_modifier(Modifier::BOLD) }
pub fn style_select() -> Style { Style::default().fg(current().select).add_modifier(Modifier::BOLD) }
pub fn style_error()  -> Style { Style::default().fg(current().error).add_modifier(Modifier::BOLD) }
pub fn style_dim()    -> Style { Style::default().fg(current().dim) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_is_first_and_default() {
        assert_eq!(THEMES[0].name, "auto");
        assert_eq!(current().name, "auto");
    }

    #[test]
    fn set_known_theme_switches() {
        assert!(set_theme("gruvbox"));
        assert_eq!(current().name, "gruvbox");
        set_theme("auto"); // restore for other tests sharing this process
    }

    #[test]
    fn set_unknown_theme_is_noop() {
        set_theme("auto");
        assert!(!set_theme("does-not-exist"));
        assert_eq!(current().name, "auto");
    }

    #[test]
    fn theme_keys_are_unique() {
        for (i, a) in THEMES.iter().enumerate() {
            for b in &THEMES[i + 1..] {
                assert_ne!(a.key, b.key, "duplicate key {} on {} and {}", a.key, a.name, b.name);
            }
        }
    }
}
