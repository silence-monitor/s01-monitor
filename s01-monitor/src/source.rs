//! Live feeds. Each source is a `Stepper`: it is told to begin, then asked to
//! do a small amount of work again and again. On a computer a worker thread
//! drives it with a short sleep between steps. A browser has no threads, so
//! there a timer drives the same steps. Either way the source itself is the
//! same code.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use web_time::{Duration, Instant};

use crate::capture::{self, Capture, Item};
use crate::decode;
use crate::sim::Simulation;
use crate::snapshot::{lock, now_ms, ReplayState, Snapshot};
use crate::stored::Ask;

/// A live feed that works in small steps.
pub trait Stepper {
    /// Opens the feed. An error here ends it with that message.
    fn begin(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String>;
    /// A little work. Called again after `interval`.
    fn tick(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String>;
    /// How long to wait between steps.
    fn interval(&self) -> Duration;
    /// Called once when the feed stops, however it stopped.
    fn end(&mut self, _snapshot: &Mutex<Snapshot>) {}
}

pub enum Kind {
    Simulation,
    /// A CANable on this serial port, or in a browser the address of the
    /// device that serves the page.
    Canable(String),
    /// A capture log played back through the decoder.
    Replay(PathBuf),
    /// A capture log already in memory, such as one picked in a browser.
    ReplayData(String, Vec<u8>),
}

enum ReplayInput {
    Path(PathBuf),
    Data(String, Vec<u8>),
}

pub struct Replay {
    input: Option<ReplayInput>,
    player: Option<Player>,
}

/// How often a chart sample is taken, on the capture's own clock.
const SAMPLE_MS: u64 = 250;

/// Feeds the capture to the decoder at its own pace. The Frames screen sets
/// `paused` and `speed` on the snapshot; every step reads them.
struct Player {
    capture: Capture,
    base: u64,
    next: usize,
    position: f64,
    sampled: u64,
    last_tick: Instant,
}

impl Player {
    fn new(capture: Capture) -> Self {
        Self {
            capture,
            base: now_ms(),
            next: 0,
            position: 0.0,
            sampled: 0,
            last_tick: Instant::now(),
        }
    }

    /// Applies every line of the capture up to `target` (milliseconds into
    /// it) and takes the chart samples that fall due on the way.
    fn play_to(&mut self, snap: &mut Snapshot, target: u64) {
        self.position = self.position.max(target as f64);
        let Player { capture, base, next, sampled, .. } = self;
        while *next < capture.entries.len() && capture.entries[*next].t_ms <= target {
            let entry = &capture.entries[*next];
            take_samples(snap, *base, sampled, entry.t_ms);
            let t_ms = *base + entry.t_ms;
            match &entry.item {
                Item::Frame(frame) => {
                    snap.tap(frame, t_ms);
                    decode::apply(snap, frame);
                }
                Item::Mark(text) => snap.mark_at(text, t_ms),
            }
            *next += 1;
        }
        take_samples(snap, *base, sampled, target.min(capture.length_ms));
    }

    /// Moves to a moment in the capture. Forward is just playing on, quickly.
    /// Backward cannot be undone, so the decoded state is rebuilt from the
    /// start of the capture up to that moment. What the person set up stays:
    /// the signals, a recording or values log in progress, and any markers
    /// they dropped themselves.
    fn seek(&mut self, snap: &mut Snapshot, target: u64) {
        let target = target.min(self.capture.length_ms);
        if target >= self.position as u64 {
            self.position = target as f64;
            self.play_to(snap, target);
            return;
        }
        let old = std::mem::take(snap);
        snap.replay = old.replay.clone();
        snap.error = old.error.clone();
        snap.set_signals(&old.signal_defs);
        let until = self.base + target;
        let from_capture: Vec<(u64, &str)> = self
            .capture
            .entries
            .iter()
            .filter_map(|entry| match &entry.item {
                Item::Mark(text) => Some((self.base + entry.t_ms, text.trim())),
                Item::Frame(_) => None,
            })
            .collect();
        let own_marks: Vec<_> = old
            .explorer
            .marks
            .iter()
            .filter(|mark| mark.t_ms <= until && !from_capture.contains(&(mark.t_ms, mark.text.as_str())))
            .cloned()
            .collect();
        self.next = 0;
        self.sampled = 0;
        self.position = target as f64;
        self.play_to(snap, target);
        snap.explorer.marks.extend(own_marks);
        snap.explorer.marks.sort_by_key(|mark| mark.t_ms);
        snap.recorder = old.recorder;
        snap.logger = old.logger;
    }
}

fn take_samples(snap: &mut Snapshot, base: u64, sampled: &mut u64, until: u64) {
    while *sampled + SAMPLE_MS <= until {
        *sampled += SAMPLE_MS;
        if snap.has_signal() {
            snap.push_sample(base + *sampled);
        }
    }
}

impl Stepper for Replay {
    fn begin(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        let (name, text) = match self.input.take() {
            Some(ReplayInput::Path(path)) => {
                let name = path
                    .file_name()
                    .map_or_else(|| "capture".to_string(), |name| name.to_string_lossy().into_owned());
                let bytes = std::fs::read(&path).map_err(|err| format!("Could not read {name}: {err}"))?;
                (name, String::from_utf8_lossy(&bytes).into_owned())
            }
            Some(ReplayInput::Data(name, bytes)) => (name, String::from_utf8_lossy(&bytes).into_owned()),
            None => return Ok(()),
        };
        let capture = capture::parse(&text).map_err(|err| format!("{name}: {err}"))?;
        lock(snapshot).replay = Some(ReplayState {
            name,
            position_ms: 0,
            length_ms: capture.length_ms,
            paused: false,
            speed: 1.0,
            finished: false,
            source_note: capture.note.clone(),
            seek_to: None,
        });
        self.player = Some(Player::new(capture));
        Ok(())
    }

    fn tick(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        let Some(player) = self.player.as_mut() else {
            return Ok(());
        };
        let elapsed = player.last_tick.elapsed();
        player.last_tick = Instant::now();
        let mut snap = lock(snapshot);
        // A request to jump to a moment. Not while a recording or values log
        // is being written: they would get lines twice, or miss some.
        let jump = snap.replay.as_mut().and_then(|state| state.seek_to.take());
        if let Some(target) = jump {
            if snap.recorder.is_none() && snap.logger.is_none() {
                player.seek(&mut snap, target);
            }
        }
        let (paused, speed, finished) = snap
            .replay
            .as_ref()
            .map_or((true, 1.0, true), |state| (state.paused, state.speed, state.finished));
        let finished = finished && player.next >= player.capture.entries.len();
        if !paused && !finished {
            player.position += elapsed.as_secs_f64() * 1000.0 * f64::from(speed);
        }
        let target = player.position as u64;
        player.play_to(&mut snap, target);
        let shown = target.min(player.capture.length_ms);
        let done = player.next >= player.capture.entries.len();
        let length = player.capture.length_ms;
        if let Some(state) = snap.replay.as_mut() {
            state.position_ms = if done { length } else { shown };
            state.finished = done;
        }
        Ok(())
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(5)
    }
}

fn make(kind: Kind, ask: Option<Receiver<Ask>>) -> Box<dyn Stepper> {
    match kind {
        Kind::Simulation => Box::new(Simulation::new()),
        Kind::Canable(port) => connect(port, ask),
        Kind::Replay(path) => Box::new(Replay {
            input: Some(ReplayInput::Path(path)),
            player: None,
        }),
        Kind::ReplayData(name, bytes) => Box::new(Replay {
            input: Some(ReplayInput::Data(name, bytes)),
            player: None,
        }),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn connect(port: String, ask: Option<Receiver<Ask>>) -> Box<dyn Stepper> {
    let (_, unused) = std::sync::mpsc::channel();
    Box::new(crate::slcan::Serial::new(&port, ask.unwrap_or(unused)))
}

#[cfg(target_arch = "wasm32")]
fn connect(url: String, ask: Option<Receiver<Ask>>) -> Box<dyn Stepper> {
    let (_, unused) = std::sync::mpsc::channel();
    Box::new(crate::socket::Socket::new(&url, ask.unwrap_or(unused)))
}

fn finish(result: Result<(), String>, stop: &AtomicBool, snapshot: &Mutex<Snapshot>) {
    if let Err(err) = result {
        if !stop.load(Ordering::SeqCst) {
            lock(snapshot).error = Some(err);
        }
    }
}

/// The running feed. On a computer it is the worker thread.
#[cfg(not(target_arch = "wasm32"))]
pub type Worker = std::thread::JoinHandle<()>;

/// In a browser the feed runs on a timer and stops by itself when `stop` is
/// set, so there is nothing to wait for.
#[cfg(target_arch = "wasm32")]
pub struct Worker;

#[cfg(target_arch = "wasm32")]
impl Worker {
    pub fn join(self) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(
    kind: Kind,
    stop: Arc<AtomicBool>,
    snapshot: Arc<Mutex<Snapshot>>,
    ask: Option<Receiver<Ask>>,
) -> Worker {
    std::thread::spawn(move || {
        let mut stepper = make(kind, ask);
        let mut result = stepper.begin(&snapshot);
        while result.is_ok() && !stop.load(Ordering::SeqCst) {
            result = stepper.tick(&snapshot);
            let wait = stepper.interval();
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        stepper.end(&snapshot);
        finish(result, &stop, &snapshot);
    })
}

#[cfg(target_arch = "wasm32")]
pub fn spawn(
    kind: Kind,
    stop: Arc<AtomicBool>,
    snapshot: Arc<Mutex<Snapshot>>,
    ask: Option<Receiver<Ask>>,
) -> Worker {
    wasm_bindgen_futures::spawn_local(async move {
        let mut stepper = make(kind, ask);
        let mut result = stepper.begin(&snapshot);
        while result.is_ok() && !stop.load(Ordering::SeqCst) {
            result = stepper.tick(&snapshot);
            gloo_timers::future::sleep(stepper.interval().max(Duration::from_millis(4))).await;
        }
        stepper.end(&snapshot);
        finish(result, &stop, &snapshot);
    });
    Worker
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::lock;

    #[test]
    fn a_replay_feeds_the_decoder_keeps_markers_and_finishes() {
        let path = std::env::temp_dir().join("s01-replay-test.log");
        std::fs::write(
            &path,
            "time:10:00:00.000 RX: STDID:00181 data:01 64 00 00 3A 02 00 00\n\
             # MARK 10:00:00.200 pressed reverse\n\
             time:10:00:00.400 RX: STDID:00181 data:02 63 00 00 3B 02 00 00\n",
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let worker = spawn(Kind::Replay(path.clone()), Arc::clone(&stop), Arc::clone(&snapshot), None);
        let started = Instant::now();
        loop {
            let done = lock(&snapshot).replay.as_ref().is_some_and(|state| state.finished);
            if done || started.elapsed() > Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        let snap = lock(&snapshot);
        let state = snap.replay.as_ref().expect("replay state");
        assert!(state.finished, "replay did not finish");
        assert_eq!(state.length_ms, 400);
        assert_eq!(snap.explorer.total, 2);
        assert_eq!(snap.explorer.marks.len(), 1);
        assert_eq!(snap.explorer.marks[0].text, "pressed reverse");
        assert_eq!(snap.soc, Some(99.0));
        assert!(snap.error.is_none());
        let _ = std::fs::remove_file(path);
    }

    fn five_second_capture() -> Capture {
        // Cell 1 reads 3900 mV, then 3910, 3920 ... every 100 ms, with a
        // marker from the capture at 250 ms.
        let mut text = String::new();
        for step in 0..=50_u32 {
            let millivolts = 3900 + step;
            text.push_str(&format!(
                "time:10:00:{:02}.{:03} RX: STDID:00185 data:{:02X} {:02X} 00 00 00 00 00 00\n",
                step * 100 / 1000,
                step * 100 % 1000,
                millivolts & 0xFF,
                millivolts >> 8
            ));
            if step == 2 {
                text.push_str("# MARK 10:00:00.250 from the capture\n");
            }
        }
        capture::parse(&text).unwrap()
    }

    fn state_of(snap: &Snapshot) -> (Option<f32>, u64, Vec<(u64, String)>, usize) {
        let marks = snap.explorer.marks.iter().map(|mark| (mark.t_ms, mark.text.clone())).collect();
        (snap.cells[0], snap.explorer.total, marks, snap.history.len())
    }

    #[test]
    fn jumping_back_gives_the_state_playing_to_that_moment_gives() {
        let mut direct = Player::new(five_second_capture());
        let mut snap_direct = Snapshot::default();
        direct.base = 1_000_000;
        direct.play_to(&mut snap_direct, 2_000);

        let mut jumped = Player::new(five_second_capture());
        jumped.base = 1_000_000;
        let mut snap_jumped = Snapshot::default();
        jumped.play_to(&mut snap_jumped, 4_500);
        assert_eq!(snap_jumped.cells[0], Some(3945.0));
        jumped.seek(&mut snap_jumped, 2_000);

        assert_eq!(state_of(&snap_direct), state_of(&snap_jumped));
        assert_eq!(snap_jumped.cells[0], Some(3920.0));
        assert_eq!(snap_jumped.explorer.marks.len(), 1);
        // And playing on from there reaches the same end as never jumping.
        jumped.play_to(&mut snap_jumped, 5_000);
        let mut straight = Player::new(five_second_capture());
        straight.base = 1_000_000;
        let mut snap_straight = Snapshot::default();
        straight.play_to(&mut snap_straight, 5_000);
        assert_eq!(state_of(&snap_straight), state_of(&snap_jumped));
    }

    #[test]
    fn jumping_forward_plays_the_lines_in_between() {
        let mut player = Player::new(five_second_capture());
        let mut snap = Snapshot::default();
        player.seek(&mut snap, 3_000);
        assert_eq!(snap.cells[0], Some(3930.0));
        assert_eq!(snap.explorer.total, 31);
        assert_eq!(snap.explorer.marks.len(), 1);
        // Past the end stops at the end.
        player.seek(&mut snap, 999_999);
        assert_eq!(snap.explorer.total, 51);
        assert_eq!(player.next, player.capture.entries.len());
    }

    #[test]
    fn a_jump_back_keeps_the_signals_and_the_markers_the_person_dropped() {
        let mut player = Player::new(five_second_capture());
        player.base = 1_000_000;
        // A marker dropped during a replay takes the replay's own clock.
        let mut snap = Snapshot {
            replay: Some(ReplayState {
                name: "test".into(),
                position_ms: 0,
                length_ms: 5_000,
                paused: true,
                speed: 1.0,
                finished: false,
                source_note: String::new(),
                seek_to: None,
            }),
            ..Snapshot::default()
        };
        let def = crate::signals::SignalDef {
            id: 0x185,
            start: 0,
            count: 2,
            signed: false,
            big_endian: false,
            scale: 1.0,
            offset: 0.0,
            unit: String::new(),
            key: String::new(),
            name: "test".into(),
        };
        snap.set_signals(std::slice::from_ref(&def));
        player.play_to(&mut snap, 3_000);
        snap.mark_now("mine at 3 s");
        player.play_to(&mut snap, 4_000);
        snap.mark_now("mine at 4 s");
        assert_eq!(snap.explorer.marks.len(), 3);

        player.seek(&mut snap, 3_500);
        let marks: Vec<&str> = snap.explorer.marks.iter().map(|mark| mark.text.as_str()).collect();
        assert_eq!(marks, ["from the capture", "mine at 3 s"], "the one at 4 s is in the future");
        assert_eq!(snap.signal_defs, vec![def]);
        assert!(!snap.signal_traces[0].history.is_empty(), "the trace was rebuilt");
    }

    #[test]
    fn the_chart_history_is_rebuilt_at_the_same_pace() {
        let mut player = Player::new(five_second_capture());
        player.base = 1_000_000;
        let mut snap = Snapshot::default();
        // A pack voltage frame so the chart has something to sample.
        let mut capture_text = String::new();
        for step in 0..=20_u32 {
            capture_text.push_str(&format!(
                "time:10:00:{:02}.{:03} RX: STDID:00181 data:01 64 00 00 3A 02 00 00\n",
                step * 250 / 1000,
                step * 250 % 1000
            ));
        }
        player.capture = capture::parse(&capture_text).unwrap();
        player.play_to(&mut snap, 4_000);
        let before = snap.history.len();
        assert!(before >= 15, "{before}");
        player.seek(&mut snap, 2_000);
        let after = snap.history.len();
        assert!((7..=9).contains(&after), "{after}");
    }

    #[test]
    fn a_recording_in_another_tools_format_replays_and_carries_its_note() {
        let path = std::env::temp_dir().join("s01-replay-candump-test.log");
        std::fs::write(
            &path,
            "(1731234567.000000) can0 181#016400003A020000\n\
             (1731234567.100000) can0 181#026300003B020000\n\
             (1731234567.150000) can0 12345678#0102\n",
        )
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let worker = spawn(Kind::Replay(path.clone()), Arc::clone(&stop), Arc::clone(&snapshot), None);
        let started = Instant::now();
        loop {
            let done = lock(&snapshot).replay.as_ref().is_some_and(|state| state.finished);
            if done || started.elapsed() > Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::SeqCst);
        worker.join().unwrap();
        let snap = lock(&snapshot);
        let state = snap.replay.as_ref().expect("replay state");
        assert!(state.finished, "the replay did not finish");
        assert_eq!(state.length_ms, 100);
        assert!(state.source_note.starts_with("Experimental: read as candump log"), "{}", state.source_note);
        assert!(state.source_note.contains("1 29-bit identifier"), "{}", state.source_note);
        assert_eq!(snap.explorer.total, 2);
        assert_eq!(snap.soc, Some(99.0), "the second frame was decoded");
        assert!(snap.error.is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_file_reports_an_error() {
        let stop = Arc::new(AtomicBool::new(false));
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let missing = std::env::temp_dir().join("s01-no-such-capture.log");
        spawn(Kind::Replay(missing), stop, Arc::clone(&snapshot), None)
            .join()
            .unwrap();
        let error = lock(&snapshot).error.clone().expect("error");
        assert!(error.contains("Could not read"), "{error}");
    }

    #[test]
    fn a_recording_taken_during_a_replay_replays_the_same_way() {
        let path = std::env::temp_dir().join("s01-record-test.log");
        let mut snap = Snapshot::default();
        let recorder = crate::capture::Recorder::create(&path, "test", 1_793_448_180_000).unwrap();
        snap.recorder = Some(Arc::new(Mutex::new(recorder)));
        let frame = crate::snapshot::Frame::new(0x280, &[0x21, 0x08, 0, 0]);
        snap.tap(&frame, 1_793_448_180_100);
        snap.mark_at("side stand down", 1_793_448_180_300);
        snap.tap(&frame, 1_793_448_180_500);
        let (_, frames, marks, error) = snap.recorder_info().expect("recording");
        assert_eq!((frames, marks), (2, 1));
        assert!(error.is_none());
        snap.recorder = None;
        let text = std::fs::read_to_string(&path).unwrap();
        let capture = capture::parse(&text).unwrap();
        assert_eq!(capture.frames, 2);
        assert_eq!(capture.length_ms, 400);
        assert_eq!(capture.entries[1].item, Item::Mark("side stand down".into()));
        let _ = std::fs::remove_file(path);
    }
}
