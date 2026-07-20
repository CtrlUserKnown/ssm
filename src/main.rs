use std::io;

use anyhow::Context;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use std::path::PathBuf;

use ssm::connect::{cli_import, cli_list, connect_direct};
use ssm::storage::keychain_available;
use ssm::tui::run_ssm;

#[derive(Parser)]
#[command(name = "ssm", version, about = "SSH session manager")]
struct Cli {
    /// Connect directly: user@host[:port]
    #[arg(short = 'c', value_name = "USER@HOST")]
    connect: Option<String>,
    /// List saved sessions
    #[arg(short = 'l', long)]
    list: bool,
    /// Import hosts from an ssh_config file (defaults to ~/.ssh/config)
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    import: Option<String>,
}

fn main() -> anyhow::Result<()> {
    // When ssh re-execs us as its SSH_ASKPASS helper, short-circuit before any
    // normal argument parsing: serve the password and exit. Detected purely by
    // the environment our own askpass server exports into the ssh child.
    if ssm::askpass::is_responder() {
        ssm::askpass::respond(); // never returns
    }

    let cli = Cli::parse();

    if let Some(spec) = cli.connect {
        return connect_direct(&spec);
    }
    if cli.list {
        return cli_list();
    }
    if let Some(path) = cli.import {
        // Empty string = flag given with no value = use the default path.
        let path = if path.is_empty() {
            None
        } else {
            Some(PathBuf::from(path))
        };
        return cli_import(path);
    }

    run_tui()
}

fn run_tui() -> anyhow::Result<()> {
    if !keychain_available() {
        eprintln!("SSM requires a keychain/secret-service backend.");
        eprintln!(
            "On Linux, ensure a secret-service daemon (e.g. gnome-keyring or kwallet) is running."
        );
        return Ok(());
    }

    let cfg = ssm::config::load();
    ssm::tui_core::theme::set_theme(&cfg.theme);

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        prev_hook(info);
    }));

    enable_raw_mode().context("could not enter raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
    }
    let _guard = Guard;

    run_ssm(&mut term, cfg)
}
