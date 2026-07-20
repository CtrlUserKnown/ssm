# ssm — SSH Session Manager

A standalone SSH session manager with a terminal user interface (TUI), built in Rust. Store, browse, and connect to SSH sessions with vim-inspired keybindings and customizable themes.

## Features

- **Session Management** — Add, edit, delete, and search SSH sessions
- **Secure Password Storage** — Passwords are stored in the OS keychain (macOS Keychain / Linux Secret Service), never in plaintext; supplied to `ssh` (and `herdr`) at connect time via `SSH_ASKPASS`, never on disk or the command line
- **Optional Biometric Unlock** — Off by default; when enabled, a fingerprint/Touch ID check gates password reveal at connect time
- **SSH Config Import** — Pull hosts straight from your existing `~/.ssh/config`
- **Tags & Group Views** — Label sessions and filter the list down to a tag
- **Jump Hosts** — Per-session `ProxyJump` support (`ssh -J`)
- **Live Reachability** — Background TCP probes show per-host latency, color-coded status, and a latency sparkline
- **Rich TUI** — Vim-style navigation (`j`/`k`, `gg`/`G`, `Ctrl-d`/`Ctrl-u`), search, and a which-key menu system
- **7 Built-in Themes** — auto, noir-cat, knew-pines (Rose Pine), catppuccin, gruvbox, nord, tokyo-night
- **Two Connection Modes** — Plain SSH or via `herdr` helper
- **CLI Interface** — Direct connection, session listing, and config import without the TUI

## Installation

### From Source

```bash
cargo build --release
```

The binary will be at `target/release/ssm`.

### Prerequisites (Linux)

- A Secret Service daemon (e.g., `gnome-keyring` or `kwallet`) for password storage
- OpenSSH 8.4+ (for the stored-password hand-off; no `sshpass` needed)
- Optional: `herdr` for the herdr connection mode
- Optional: `fprintd` (`fprintd-verify`) for biometric unlock
- Optional: `xclip` or `xsel` for clipboard yank support

## Usage

```bash
# Launch the TUI
ssm

# List saved sessions
ssm --list

# Connect directly
ssm -c user@host
ssm -c user@host:2222

# Import hosts from your SSH config
ssm --import                    # ~/.ssh/config
ssm --import /path/to/ssh_config
```

### CLI Flags

| Flag | Description |
|---|---|
| *(none)* | Launch the full-screen TUI |
| `-c USER@HOST[:PORT]` | Connect directly to a host |
| `-l`, `--list` | Print saved sessions as a table |
| `--import [PATH]` | Import hosts from an ssh_config file (default `~/.ssh/config`); skips names that already exist |
| `--version` | Print version |
| `--help` | Print help |

## Keybindings

| Key | Action |
|---|---|
| `j`/`k` | Move down/up (supports numeric prefixes, e.g. `5j`) |
| `gg`/`G` | Go to first/last session |
| `Ctrl-d`/`Ctrl-u` | Half-page scroll |
| `Ctrl-f`/`Ctrl-b` | Full-page scroll |
| `Enter` | Connect to selected session |
| `a` | Add new session |
| `e` | Edit selected session |
| `D`/`dd` | Delete selected session |
| `y` | Yank (copy) host to clipboard |
| `/` | Search/filter sessions (matches tags too) |
| `T` | Filter by tag (group view); `Esc` clears the filter |
| `u` | Reload sessions from disk |
| `Space` | Open which-key menu (delete, yank, tag filter, import, settings) |
| `?` | Toggle help screen |
| `q` | Quit |

## Configuration

Configuration files are created automatically at:

| File | Purpose |
|---|---|
| `~/.config/ssm/config.toml` | Preferences (theme, herdr, probe, biometric toggles) |
| `~/.config/ssm/sessions.json` | Saved sessions |

Override the config directory with the `DOTS_SSM_DIR` environment variable.

### config.toml

```toml
use_herdr = true
theme = "auto"
probe = true             # background reachability probing + latency in the list
biometric_unlock = false # require a biometric check before revealing a password
```

## Tags, Jump Hosts & Reachability

**Tags** — Sessions carry free-form tags (set them in the add/edit form as a
comma- or space-separated list). Tags show as dim chips in the list, are matched
by `/` search, and power a group view: press `T` to pick a tag and filter the
list down to it. `Esc` clears the filter.

**Jump hosts** — Each session has an optional `ProxyJump` field. When set, ssm
passes it to ssh as `-J <jump>` (e.g. `bastion` or `user@bastion:2222`). Imported
`~/.ssh/config` hosts carry their `ProxyJump` directive across automatically.

**Reachability** — When `probe` is on, ssm runs a background TCP connect to each
host's port every few seconds and shows a status dot, the round-trip latency, and
a small latency-history sparkline:

- `●` green — reachable, low latency (< 50 ms)
- `●` yellow — reachable, higher latency (< 200 ms)
- `●` red — unreachable
- `○` gray — not probed yet

Toggle probing at runtime via `Space` → Settings → `p`, or with `probe = false`
in `config.toml`.

## Stored Passwords, herdr & Biometric Unlock

### How the password reaches ssh (and herdr)

When a session has a stored password, ssm hands it to OpenSSH through the
`SSH_ASKPASS` mechanism (`SSH_ASKPASS_REQUIRE=force`, OpenSSH 8.4+): ssm points
`SSH_ASKPASS` at itself and serves the secret over a `0600` Unix socket guarded
by a one-time nonce. Nothing is written to disk and the password never appears
on a command line or in a persistent environment variable. Because `herdr
--remote` shells out to system `ssh`, the same mechanism supplies the password
to herdr connections too — so stored passwords now work in both modes (no
`sshpass` required).

Notes:
- For plain `ssh`, ssm adds `-o StrictHostKeyChecking=accept-new`, so first-time
  connections to new hosts still work; a *changed* host key still aborts.
- `herdr --remote` takes only a target (no ssh flags), so per-session port /
  `ProxyJump` must live in `~/.ssh/config`, and the host should already be in
  `known_hosts`.

### Biometric unlock (optional)

Off by default. Enable via `Space` → Settings → `b`, or `biometric_unlock = true`.
When on, ssm requires a biometric check before revealing a stored password at
connect time. Passwords are loaded lazily (only at connect), so the prompt
actually gates access.

- **Linux:** uses `fprintd-verify` — a fingerprint *presence gate* in front of
  the Secret Service release. (Not cryptographically bound to the secret.)
- **macOS:** Touch ID is not wired up yet. A real implementation needs
  `LocalAuthentication` and a **stably code-signed** binary so the Secure-Enclave
  keychain item survives rebuilds. Use `scripts/sign-macos.sh` with a free
  self-signed "Code Signing" certificate for personal use, or a Developer ID
  certificate + notarization to distribute. Until it lands, enabling biometric
  on macOS falls back to a plain keychain read with a warning.

If biometric is enabled but no verifier is available, ssm warns and proceeds
rather than locking you out of your own hosts.

## Themes

Cycle through themes live from the TUI via `Space` → Settings → Theme. Available themes:

- `auto` — matches terminal colors
- `noir-cat`
- `knew-pines` — Rose Pine inspired
- `catppuccin`
- `gruvbox`
- `nord`
- `tokyo-night`

## License

See [LICENSE](LICENSE) for details.
