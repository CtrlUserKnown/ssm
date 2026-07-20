//! Background reachability probing.
//!
//! ssm doesn't hold live SSH connections, so it can't show in-session health the
//! way a persistent client can. What it *can* do cheaply is a periodic TCP
//! connect to each host's SSH port and time it — enough to color the list by
//! reachability and latency, adapted from essh's fleet prober.
//!
//! The probing runs on a plain std thread (no async runtime): the main TUI loop
//! hands it the current host list via a shared mutex and drains fresh results
//! from an [`mpsc`] channel each frame. Results are keyed by `"host:port"`.

use std::collections::{HashMap, VecDeque};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::style::Color;

/// How many past latency samples to keep per host (drives the sparkline).
const HISTORY_CAP: usize = 12;

/// A single probe outcome: `Some(ms)` reachable with round-trip latency, `None`
/// unreachable (refused, timed out, or unresolvable).
pub type Latency = Option<f64>;

/// The `"host:port"` key used to look up probe state.
pub fn key(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// Per-host reachability state accumulated on the main thread.
#[derive(Clone, Debug, Default)]
pub struct ProbeState {
    /// Most recent probe outcome, or `None` if never probed / unreachable.
    pub last: Latency,
    /// Whether we've heard back at least once (distinguishes "probing…" from
    /// "unreachable").
    pub seen: bool,
    /// Recent latency samples (reachable probes only), oldest first.
    pub history: VecDeque<f64>,
}

impl ProbeState {
    fn record(&mut self, latency: Latency) {
        self.seen = true;
        self.last = latency;
        if let Some(ms) = latency {
            if self.history.len() == HISTORY_CAP {
                self.history.pop_front();
            }
            self.history.push_back(ms);
        }
    }

    /// A status glyph + color for the list row.
    pub fn indicator(&self) -> (&'static str, Color) {
        match (self.seen, self.last) {
            (false, _) => ("○", Color::DarkGray),         // not yet probed
            (true, None) => ("●", Color::Red),            // unreachable
            (true, Some(ms)) => ("●", latency_color(ms)), // reachable
        }
    }

    /// A tiny unicode sparkline of recent latencies, or empty when we have none.
    pub fn sparkline(&self) -> String {
        sparkline(self.history.iter().copied())
    }
}

/// Netwatch-style latency thresholds: green < 50 ms, yellow < 200 ms, else red.
pub fn latency_color(ms: f64) -> Color {
    if ms < 50.0 {
        Color::Green
    } else if ms < 200.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

const SPARK_TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render an iterator of latencies as block-tick characters, scaled to the max.
fn sparkline(samples: impl Iterator<Item = f64>) -> String {
    let vals: Vec<f64> = samples.collect();
    if vals.is_empty() {
        return String::new();
    }
    let max = vals.iter().cloned().fold(0.0_f64, f64::max).max(1.0);
    vals.iter()
        .map(|&v| {
            let idx = ((v / max) * (SPARK_TICKS.len() - 1) as f64).round() as usize;
            SPARK_TICKS[idx.min(SPARK_TICKS.len() - 1)]
        })
        .collect()
}

/// Owns the background probe thread and the channel of results.
pub struct Prober {
    targets: Arc<Mutex<Vec<(String, u16)>>>,
    rx: Receiver<(String, u16, Latency)>,
    // Kept alive for the process; the thread exits when the receiver drops.
    _handle: thread::JoinHandle<()>,
}

impl Prober {
    /// Spawn the prober. `interval` is the pause between full sweeps; `timeout`
    /// bounds each individual TCP connect.
    pub fn spawn(interval: Duration, timeout: Duration) -> Self {
        let targets: Arc<Mutex<Vec<(String, u16)>>> = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx): (Sender<_>, Receiver<_>) = mpsc::channel();
        let thread_targets = Arc::clone(&targets);

        let handle = thread::Builder::new()
            .name("ssm-prober".to_string())
            .spawn(move || probe_loop(thread_targets, tx, interval, timeout))
            .expect("spawn prober thread");

        Self {
            targets,
            rx,
            _handle: handle,
        }
    }

    /// Replace the set of hosts being probed.
    pub fn set_targets(&self, new_targets: Vec<(String, u16)>) {
        if let Ok(mut t) = self.targets.lock() {
            *t = new_targets;
        }
    }

    /// Pull all probe results that have arrived since the last call.
    pub fn drain(&self) -> Vec<(String, u16, Latency)> {
        self.rx.try_iter().collect()
    }
}

fn probe_loop(
    targets: Arc<Mutex<Vec<(String, u16)>>>,
    tx: Sender<(String, u16, Latency)>,
    interval: Duration,
    timeout: Duration,
) {
    loop {
        let snapshot = targets.lock().map(|t| t.clone()).unwrap_or_default();
        for (host, port) in &snapshot {
            let latency = probe_one(host, *port, timeout);
            // A send error means the main thread (and receiver) is gone — exit.
            if tx.send((host.clone(), *port, latency)).is_err() {
                return;
            }
        }
        thread::sleep(interval);
    }
}

/// Probe a single host: resolve, TCP-connect with a timeout, and measure the
/// round trip. Returns `None` on any failure.
fn probe_one(host: &str, port: u16, timeout: Duration) -> Latency {
    let start = Instant::now();
    // `to_socket_addrs` performs DNS resolution (may block — fine off-thread).
    let addrs = (host, port).to_socket_addrs().ok()?;
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            return Some(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    None
}

/// Fold a batch of drained results into the per-host state map.
pub fn apply_results(
    states: &mut HashMap<String, ProbeState>,
    results: Vec<(String, u16, Latency)>,
) {
    for (host, port, latency) in results {
        states.entry(key(&host, port)).or_default().record(latency);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_capped() {
        let mut s = ProbeState::default();
        for i in 0..(HISTORY_CAP + 5) {
            s.record(Some(i as f64));
        }
        assert_eq!(s.history.len(), HISTORY_CAP);
        // Oldest samples dropped; newest retained.
        assert_eq!(*s.history.back().unwrap(), (HISTORY_CAP + 4) as f64);
    }

    #[test]
    fn unreachable_does_not_grow_history() {
        let mut s = ProbeState::default();
        s.record(Some(20.0));
        s.record(None);
        assert_eq!(s.history.len(), 1);
        assert!(s.seen);
        assert!(s.last.is_none());
    }

    #[test]
    fn indicator_states() {
        let mut s = ProbeState::default();
        assert_eq!(s.indicator().1, Color::DarkGray); // unprobed
        s.record(None);
        assert_eq!(s.indicator().1, Color::Red); // unreachable
        s.record(Some(10.0));
        assert_eq!(s.indicator().1, Color::Green); // fast
    }

    #[test]
    fn latency_thresholds() {
        assert_eq!(latency_color(49.9), Color::Green);
        assert_eq!(latency_color(50.0), Color::Yellow);
        assert_eq!(latency_color(200.0), Color::Red);
    }

    #[test]
    fn sparkline_scales_and_lengths() {
        assert_eq!(sparkline(std::iter::empty()), "");
        let s = sparkline([1.0, 5.0, 10.0].into_iter());
        assert_eq!(s.chars().count(), 3);
        // Largest sample maps to the tallest tick.
        assert!(s.ends_with('█'));
    }

    #[test]
    fn apply_results_keys_by_host_port() {
        let mut states = HashMap::new();
        apply_results(&mut states, vec![("h".to_string(), 22, Some(5.0))]);
        assert!(states.contains_key("h:22"));
        assert_eq!(states["h:22"].last, Some(5.0));
    }

    #[test]
    fn probe_unreachable_reserved_ip() {
        // TEST-NET-1 (192.0.2.0/24) is reserved and unroutable.
        let latency = probe_one("192.0.2.1", 1, Duration::from_millis(150));
        assert!(latency.is_none());
    }
}
