//! `ssm` — a standalone SSH session manager.
//!
//! Formerly a subcommand of `dots`; now its own binary. It shares only the
//! `tui-core` rendering library with `dots`, and reads the `[ssm]` section of
//! the dots `settings.toml` for its one preference (herdr vs. plain ssh).

pub mod config;
pub mod connect;
pub mod storage;
pub mod tui;

/// Shared ratatui chrome + theme, copied from the dots `tui-core` crate so ssm
/// stands alone with no cross-repo dependency.
pub mod tui_core;
