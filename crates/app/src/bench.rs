//! `--bench` measurements of list scrolling. Once the list is filled, QML
//! drives the scroll and reports events here; the numbers are printed to
//! stdout.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};

use crate::session::{self, Change};

#[cxx_qt::bridge]
pub mod qobject {
    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, enabled)]
        type BenchProbe = super::BenchProbeRust;
    }

    extern "RustQt" {
        #[qinvokable]
        #[cxx_name = "firstFrame"]
        fn first_frame(self: Pin<&mut BenchProbe>);

        /// The library's first view is shown; time to start scrolling.
        #[qsignal]
        #[cxx_name = "listFilled"]
        fn list_filled(self: Pin<&mut BenchProbe>);

        #[qinvokable]
        #[cxx_name = "beginPhase"]
        fn begin_phase(self: Pin<&mut BenchProbe>, name: &QString);

        /// One animation tick; `seconds` is the time since the previous one.
        #[qinvokable]
        fn frame(self: Pin<&mut BenchProbe>, seconds: f64);

        #[qinvokable]
        fn finish(self: Pin<&mut BenchProbe>, refresh_hz: f64);
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    impl cxx_qt::Threading for BenchProbe {}
    impl cxx_qt::Initialize for BenchProbe {}
}

pub struct BenchProbeRust {
    enabled: bool,
    first_frame_seen: bool,
    list_seen: bool,
    phase: Option<Phase>,
    phases: Vec<Phase>,
}

struct Phase {
    name: String,
    frames: Vec<f64>,
}

impl Default for BenchProbeRust {
    fn default() -> Self {
        Self {
            enabled: crate::config().bench,
            first_frame_seen: false,
            list_seen: false,
            phase: None,
            phases: Vec::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::BenchProbe {
    fn initialize(self: Pin<&mut Self>) {
        if !self.enabled {
            return;
        }
        for change in [Change::View, Change::ViewRefreshed] {
            session::on_change(self.qt_thread(), change, Self::view_shown);
        }
    }
}

impl qobject::BenchProbe {
    fn first_frame(mut self: Pin<&mut Self>) {
        if !self.enabled || self.first_frame_seen {
            return;
        }
        self.as_mut().rust_mut().first_frame_seen = true;
        println!("first frame:      {:>8.1} ms", since_start_ms());
        print_rss("rss at first frame");
    }

    fn view_shown(mut self: Pin<&mut Self>) {
        if self.list_seen {
            return;
        }
        self.as_mut().rust_mut().list_seen = true;
        println!(
            "list filled:      {:>8.1} ms ({} rows)",
            since_start_ms(),
            session::view().len()
        );
        print_rss("rss with the list");
        self.list_filled();
    }

    fn begin_phase(mut self: Pin<&mut Self>, name: &cxx_qt_lib::QString) {
        let mut rust = self.as_mut().rust_mut();
        let rust = &mut *rust;
        if let Some(done) = rust.phase.take() {
            rust.phases.push(done);
        }
        rust.phase = Some(Phase {
            name: name.to_string(),
            frames: Vec::with_capacity(4096),
        });
    }

    fn frame(mut self: Pin<&mut Self>, seconds: f64) {
        if let Some(phase) = &mut self.as_mut().rust_mut().phase {
            phase.frames.push(seconds);
        }
    }

    fn finish(mut self: Pin<&mut Self>, refresh_hz: f64) {
        let mut rust = self.as_mut().rust_mut();
        let rust = &mut *rust;
        if let Some(done) = rust.phase.take() {
            rust.phases.push(done);
        }
        let hz = if refresh_hz > 0.0 { refresh_hz } else { 60.0 };
        println!("refresh rate:     {hz:>8.1} Hz (dropped = a frame longer than 1.5 intervals)");
        println!(
            "{:<12} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "phase", "frames", "dropped", "p50 ms", "p95 ms", "p99 ms", "max ms"
        );
        for phase in &rust.phases {
            let stats = FrameStats::new(&phase.frames, hz);
            println!(
                "{:<12} {:>7} {:>8} {:>8.2} {:>8.2} {:>8.2} {:>8.2}",
                phase.name, stats.frames, stats.dropped, stats.p50, stats.p95, stats.p99, stats.max
            );
        }
        print_rss("rss at the end");
    }
}

fn since_start_ms() -> f64 {
    crate::config().started.elapsed().as_secs_f64() * 1000.0
}

fn print_rss(label: &str) {
    match rss() {
        Some(bytes) => println!("{label}: {:>8.1} MB", bytes as f64 / 1_048_576.0),
        None => println!("{label}: unavailable"),
    }
}

#[cfg(target_os = "linux")]
fn rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(target_os = "macos")]
fn rss() -> Option<u64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8(out.stdout).ok()?.trim().parse().ok()?;
    Some(kb * 1024)
}

#[cfg(windows)]
#[expect(unsafe_code, reason = "reading the working set is a Win32 call")]
fn rss() -> Option<u64> {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = u32::try_from(std::mem::size_of_val(&counters)).ok()?;
    // SAFETY: `counters` is a live, writable struct of the size passed, and
    // the current-process pseudo handle needs no closing.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) }.ok()?;
    u64::try_from(counters.WorkingSetSize).ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn rss() -> Option<u64> {
    None
}

#[derive(Debug, PartialEq)]
struct FrameStats {
    frames: usize,
    dropped: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

impl FrameStats {
    /// `frames` are intervals in seconds; the results are in milliseconds.
    fn new(frames: &[f64], refresh_hz: f64) -> Self {
        let mut ms: Vec<f64> = frames.iter().map(|s| s * 1000.0).collect();
        ms.sort_by(f64::total_cmp);
        let budget = 1500.0 / refresh_hz;
        let pick = |q: f64| {
            if ms.is_empty() {
                return 0.0;
            }
            let i = ((ms.len() - 1) as f64 * q).round() as usize;
            ms[i]
        };
        Self {
            frames: ms.len(),
            dropped: ms.iter().filter(|&&f| f > budget).count(),
            p50: pick(0.50),
            p95: pick(0.95),
            p99: pick(0.99),
            max: ms.last().copied().unwrap_or(0.0),
        }
    }
}

#[cfg(test)]
#[path = "tests/bench_test.rs"]
mod tests;
