//! The slcan line protocol: `t<id3><dlc><data>` lines in, `r1838` style
//! request lines out. The same conversation runs over a CANable on a serial
//! port and, in a browser, over a WebSocket to the device on the scooter, so
//! the conversation lives in `Session` and the two ends only move bytes.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Mutex;

use web_time::{Duration, Instant};

use crate::clock::{self, Moment};
use crate::decode;
use crate::snapshot::{lock, DebugState, Frame, Snapshot};
use crate::stored::{Ask, Job, Reply, DEBUG_OFF_LINE, DEBUG_ON_LINE};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    Listen,
    Exchange,
}

/// How long a parameter read waits for its reply before asking again, and how
/// many times it asks again before giving up.
const REPLY_WAIT: Duration = Duration::from_millis(2000);
const REPLY_RETRIES: u8 = 2;
/// The event log is read a page at a time and ends at record 0. If a page's
/// replies stop for this long the read is given up with what it has, and if
/// nothing at all comes back it is given up after a little longer.
const EVENTS_QUIET: Duration = Duration::from_millis(1500);
const EVENTS_FIRST_REPLY: Duration = Duration::from_millis(3000);
/// The bus counts as asleep after this long with no frame, and the first frame
/// after that is the bus waking.
const BUS_QUIET: Duration = Duration::from_millis(1000);
/// After the bus wakes the app first looks, for this long, to see whether the
/// ECU already reports debug mode (`0x28A` byte 3 bit 0). Only if it does not is
/// debug mode sent. After a send it waits this long for the ECU to say it is on,
/// and sends once more if it did not, and in all it sends at most this many times.
const DEBUG_LOOK: Duration = Duration::from_millis(150);
const DEBUG_CONFIRM: Duration = Duration::from_millis(1000);
const DEBUG_SENDS: u8 = 2;

/// Where the app has got to with debug mode since the bus last woke.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DebugPhase {
    /// Nothing to do: before the first frame, or the bus is asleep.
    Idle,
    /// Watching for the ECU to report debug mode, since this moment.
    Looking(Instant),
    /// Sent; waiting for the ECU to confirm. Last sent at this moment, this many times.
    Waiting(Instant, u8),
    /// Confirmed, or given up.
    Done,
}

/// One conversation with the bus. After the first frame, one remote request
/// asks for the pack id. A stored read sends on the same channel and waits
/// for the matching reply. Lines to send are pushed onto `out`; the caller
/// writes them.
pub struct Session {
    pending: Vec<u8>,
    gate: Gate,
    job: Option<Job>,
    asked_pack: bool,
    sampled: Instant,
    /// For the read in progress: when it last heard its device, how many
    /// replies that made, and how often its request has been sent again.
    heard_at: Instant,
    heard: u32,
    retries: u8,
    /// The next page of the event log, and when to send it.
    delayed: Option<(Instant, String)>,
    /// A request to set the battery clock that has been sent: when, and to what.
    /// The battery's own clock frame must then show that time.
    clock_check: Option<(Instant, Moment)>,
    /// When the newest frame arrived, and whether debug mode is on from here.
    last_frame: Option<Instant>,
    /// The debug switch as this session last saw it, to notice it being flipped.
    debug_seen: bool,
    debug_checked: Instant,
    debug_phase: DebugPhase,
    /// True once this session has sent debug on since the bus woke. Only then
    /// does it send debug off when it ends.
    debug_sent: bool,
}

impl Session {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            gate: Gate::Listen,
            job: None,
            asked_pack: false,
            sampled: Instant::now(),
            heard_at: Instant::now(),
            heard: 0,
            retries: 0,
            delayed: None,
            clock_check: None,
            last_frame: None,
            debug_seen: false,
            debug_checked: Instant::now(),
            debug_phase: DebugPhase::Idle,
            debug_sent: false,
        }
    }

    /// Between bytes: start a read the person asked for, and take the chart
    /// sample every 250 ms.
    pub fn poll(&mut self, snapshot: &Mutex<Snapshot>, ask: &Receiver<Ask>, out: &mut Vec<String>) {
        if self.gate == Gate::Listen {
            match ask.try_recv() {
                Ok(Ask::SetClock(moment)) => self.start_clock_set(moment, snapshot, out),
                Ok(request) => {
                    let started = Job::begin(request);
                    {
                        let mut snap = lock(snapshot);
                        snap.read_busy = true;
                        let note = started.reading_note();
                        if request == Ask::Events {
                            snap.event_note = note.into();
                            snap.event_rows.clear();
                            snap.events_ready = true;
                        } else {
                            snap.param_note = note.into();
                        }
                    }
                    if let Some(line) = started.opening_line() {
                        out.push(line.to_string());
                        self.job = Some(started);
                        self.gate = Gate::Exchange;
                        self.heard_at = Instant::now();
                        self.heard = 0;
                        self.retries = 0;
                        self.delayed = None;
                    } else {
                        publish(snapshot, &started);
                        lock(snapshot).read_busy = false;
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
            }
        }
        self.check_stalled(Instant::now(), snapshot, out);
        self.check_clock(Instant::now(), snapshot);
        // The debug switch is looked at every 20 ms, not on every byte.
        let now = Instant::now();
        if now.saturating_duration_since(self.debug_checked) >= Duration::from_millis(20) {
            self.debug_checked = now;
            self.check_debug(now, snapshot, out);
        }
        if self.sampled.elapsed() >= Duration::from_millis(250) {
            let mut snap = lock(snapshot);
            if snap.has_signal() {
                snap.push_sample(crate::snapshot::now_ms());
            }
            self.sampled = Instant::now();
        }
    }

    /// A frame has arrived. The first one, and the first after the bus has been
    /// quiet for a second, is the bus waking. The scooter forgets debug mode when
    /// it sleeps, so if the person has the debug switch on, a new look starts.
    /// The app first looks for the ECU's own report of debug mode (`0x28A` byte 3
    /// bit 0): if it is already set nothing is sent, and if it is not set after
    /// a short look, `check_debug` sends it. With the switch off nothing is ever
    /// sent.
    fn note_frame(&mut self, now: Instant, frame: &Frame, snapshot: &Mutex<Snapshot>) {
        let waking = self.last_frame.is_none_or(|at| now.saturating_duration_since(at) > BUS_QUIET);
        self.last_frame = Some(now);
        if waking {
            let mut snap = lock(snapshot);
            self.debug_sent = false;
            self.debug_phase = if snap.debug_wanted { DebugPhase::Looking(now) } else { DebugPhase::Idle };
            snap.debug = DebugState::Idle;
        }
        let debug_on = frame.id == 0x28A && frame.len >= 4 && frame.data[3] & 0x01 == 0x01;
        if debug_on {
            match self.debug_phase {
                DebugPhase::Looking(_) => {
                    self.debug_phase = DebugPhase::Done;
                    lock(snapshot).debug = DebugState::AlreadyOn;
                }
                DebugPhase::Waiting(..) => {
                    self.debug_phase = DebugPhase::Done;
                    lock(snapshot).debug = DebugState::Confirmed;
                }
                DebugPhase::Idle | DebugPhase::Done => {}
            }
        }
    }

    /// Between bytes. Follows the person's debug switch: switched on while the
    /// scooter is awake starts a look; switched off sends debug off if the app
    /// had sent debug on. With the switch on it sends debug on if the ECU has not
    /// reported it, and again once if it still has not after a second, and gives
    /// up after that and says so. When the bus has been quiet for a second the
    /// scooter is asleep and has forgotten debug mode, so everything starts again
    /// at the next wake. Nothing is ever sent to a sleeping bus.
    fn check_debug(&mut self, now: Instant, snapshot: &Mutex<Snapshot>, out: &mut Vec<String>) {
        let wanted = lock(snapshot).debug_wanted;
        let heard = self.last_frame.is_some();
        let awake = self.last_frame.is_some_and(|at| now.saturating_duration_since(at) <= BUS_QUIET);
        if !wanted {
            // Switched off, or never on. If the app had sent debug on, switch it off.
            if self.debug_phase != DebugPhase::Idle || self.debug_sent {
                if self.debug_sent && awake {
                    out.push(DEBUG_OFF_LINE.to_string());
                }
                self.debug_sent = false;
                self.debug_phase = DebugPhase::Idle;
                lock(snapshot).debug = DebugState::Idle;
            }
            self.debug_seen = false;
            return;
        }
        if heard && !awake {
            if self.debug_phase != DebugPhase::Idle || self.debug_sent {
                self.debug_phase = DebugPhase::Idle;
                self.debug_sent = false;
                lock(snapshot).debug = DebugState::Idle;
            }
            return;
        }
        if !self.debug_seen {
            // Just switched on while the scooter is awake: start looking now.
            self.debug_seen = true;
            if awake && self.debug_phase == DebugPhase::Idle {
                self.debug_phase = DebugPhase::Looking(now);
            }
        }
        match self.debug_phase {
            DebugPhase::Looking(since) if now.saturating_duration_since(since) >= DEBUG_LOOK => {
                out.push(DEBUG_ON_LINE.to_string());
                self.debug_sent = true;
                self.debug_phase = DebugPhase::Waiting(now, 1);
                lock(snapshot).debug = DebugState::Sent;
            }
            DebugPhase::Waiting(since, sends) if now.saturating_duration_since(since) >= DEBUG_CONFIRM => {
                if sends < DEBUG_SENDS {
                    out.push(DEBUG_ON_LINE.to_string());
                    self.debug_phase = DebugPhase::Waiting(now, sends + 1);
                } else {
                    self.debug_phase = DebugPhase::Done;
                    lock(snapshot).debug = DebugState::Unconfirmed;
                }
            }
            _ => {}
        }
    }

    /// The conversation is ending (disconnect, or the app closing). Debug off is
    /// sent if this session sent debug on and the bus is still awake; a sleeping
    /// bus is not sent to. If the ECU already had debug mode on, the app leaves
    /// it as it was. The switch goes back to off, so the next connection only
    /// listens.
    pub fn disconnecting(&mut self, snapshot: &Mutex<Snapshot>, out: &mut Vec<String>) {
        let awake = self.last_frame.is_some_and(|at| at.elapsed() <= BUS_QUIET);
        if self.debug_sent && awake {
            out.push(DEBUG_OFF_LINE.to_string());
        }
        self.debug_sent = false;
        self.debug_phase = DebugPhase::Idle;
        let mut snap = lock(snapshot);
        snap.debug = DebugState::Idle;
        snap.debug_wanted = false;
    }

    /// Sends the one write the app makes: the frame that sets the battery clock.
    /// It is only ever started by a person pressing a button and confirming.
    fn start_clock_set(&mut self, moment: Moment, snapshot: &Mutex<Snapshot>, out: &mut Vec<String>) {
        let mut snap = lock(snapshot);
        match moment.set_line() {
            Some(line) => {
                out.push(line);
                self.clock_check = Some((Instant::now(), moment));
                snap.clock_note = clock::NOTE_SETTING.into();
            }
            None => snap.clock_note = clock::NOTE_NO_TIME.into(),
        }
    }

    /// After a set: ends the check when the battery's clock frame shows the time
    /// that was sent (plus the seconds since), or says it does not.
    fn check_clock(&mut self, now: Instant, snapshot: &Mutex<Snapshot>) {
        let Some((sent, target)) = self.clock_check else {
            return;
        };
        let elapsed = now.saturating_duration_since(sent);
        let mut snap = lock(snapshot);
        if let Some(battery) = snap.battery_clock {
            let expected = target.seconds() + i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX);
            if (battery.seconds() - expected).abs() <= clock::CHECK_TOLERANCE_S {
                snap.clock_note = clock::NOTE_MATCHES.into();
                self.clock_check = None;
                return;
            }
        }
        if elapsed >= Duration::from_secs(clock::CHECK_WAIT_S) {
            snap.clock_note = clock::NOTE_DIFFERS.into();
            self.clock_check = None;
        }
    }

    /// Looks at the read in progress and ends it when its device has gone
    /// quiet. An event-log read is over when the replies stop. A parameter
    /// read sends its request again, twice, and then gives up with what it has.
    /// Without this a lost reply left the app "reading" for ever, and every
    /// other read refused to start.
    fn check_stalled(&mut self, now: Instant, snapshot: &Mutex<Snapshot>, out: &mut Vec<String>) {
        let Some(job) = self.job.as_ref() else {
            return;
        };
        // The pause between two pages of the event log is not the device going
        // quiet. When it is over the next page goes out and the wait for its
        // replies starts from that moment.
        if let Some((due, _)) = &self.delayed {
            if now >= *due {
                if let Some((_, line)) = self.delayed.take() {
                    out.push(line);
                }
                self.heard_at = now;
            }
            return;
        }
        let heard = job.replies();
        if heard != self.heard {
            self.heard = heard;
            self.heard_at = now;
            return;
        }
        let quiet = now.saturating_duration_since(self.heard_at);
        if job.ask() == Ask::Events {
            // The read ends by itself at record 0, so replies that stop before
            // that mean it stopped part way.
            if (heard > 0 && quiet >= EVENTS_QUIET) || (heard == 0 && quiet >= EVENTS_FIRST_REPLY) {
                self.finish(snapshot, false);
            }
        } else if quiet >= REPLY_WAIT {
            if self.retries < REPLY_RETRIES {
                self.retries += 1;
                self.heard_at = now;
                if let Some(line) = job.retry_line() {
                    out.push(line);
                }
            } else {
                self.finish(snapshot, false);
            }
        }
    }

    /// Ends the read in progress with what it has. `complete` is false when
    /// the device stopped answering.
    fn finish(&mut self, snapshot: &Mutex<Snapshot>, complete: bool) {
        self.delayed = None;
        if let Some(job) = self.job.take() {
            publish(snapshot, &job);
            let mut snap = lock(snapshot);
            if !complete {
                match job.ask() {
                    Ask::Events => snap.event_note = job.stalled_note(),
                    // A clock set never has a job.
                    Ask::Ecu | Ask::Battery | Ask::SetClock(_) => snap.param_note = job.stalled_note(),
                }
            }
            snap.read_busy = false;
        }
        self.gate = Gate::Listen;
    }

    /// One byte arrived. A finished line that is a frame goes to the decoder.
    pub fn on_byte(&mut self, byte: u8, snapshot: &Mutex<Snapshot>, out: &mut Vec<String>) {
        let Some(frame) = push_byte(&mut self.pending, byte) else {
            return;
        };
        {
            let mut snap = lock(snapshot);
            snap.tap(&frame, crate::snapshot::now_ms());
            decode::apply(&mut snap, &frame);
        }
        self.note_frame(Instant::now(), &frame, snapshot);
        if !self.asked_pack {
            self.asked_pack = true;
            out.push("r1838\r".to_string());
        }
        if self.gate == Gate::Exchange {
            let mut finished = false;
            if let Some(current) = self.job.as_mut() {
                match current.on_frame(frame.id, &frame.data[..frame.len]) {
                    Reply::Wait => {
                        if current.ask() == Ask::Events {
                            show_events(snapshot, current);
                        }
                    }
                    Reply::Send(line) => {
                        if current.ask() == Ask::Events {
                            show_events(snapshot, current);
                        }
                        out.push(line.to_string());
                    }
                    Reply::SendAfter(line, wait) => {
                        show_events(snapshot, current);
                        self.delayed = Some((Instant::now() + wait, line));
                    }
                    Reply::Done => {
                        publish(snapshot, current);
                        finished = true;
                    }
                }
            }
            if finished {
                lock(snapshot).read_busy = false;
                self.job = None;
                self.delayed = None;
                self.gate = Gate::Listen;
            }
        }
    }
}

/// A CANable on a serial port. It stays open and acknowledging.
#[cfg(not(target_arch = "wasm32"))]
pub struct Serial {
    name: String,
    port: Option<Box<dyn serialport::SerialPort>>,
    ask: Receiver<Ask>,
    session: Session,
}

#[cfg(not(target_arch = "wasm32"))]
impl Serial {
    pub fn new(name: &str, ask: Receiver<Ask>) -> Self {
        Self {
            name: name.to_string(),
            port: None,
            ask,
            session: Session::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::source::Stepper for Serial {
    fn begin(&mut self, _snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        let mut port = serialport::new(&self.name, 115_200)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|err| format!("Could not open {}: {err}", self.name))?;
        for command in [b"C\r".as_slice(), b"S5\r".as_slice(), b"O\r".as_slice()] {
            write_port(&mut port, command, &self.name)?;
            std::thread::sleep(Duration::from_millis(30));
        }
        self.port = Some(port);
        Ok(())
    }

    fn tick(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        let Some(port) = self.port.as_mut() else {
            return Ok(());
        };
        let mut out = Vec::new();
        self.session.poll(snapshot, &self.ask, &mut out);
        let mut byte = [0u8; 1];
        match port.read(&mut byte) {
            Ok(0) => {}
            Ok(_) => self.session.on_byte(byte[0], snapshot, &mut out),
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {}
            Err(err) => return Err(format!("Serial read failed on {}: {err}", self.name)),
        }
        for line in out {
            write_port(port, line.as_bytes(), &self.name)?;
        }
        Ok(())
    }

    fn interval(&self) -> Duration {
        Duration::ZERO
    }

    fn end(&mut self, snapshot: &Mutex<Snapshot>) {
        if let Some(port) = self.port.as_mut() {
            let mut out = Vec::new();
            self.session.disconnecting(snapshot, &mut out);
            for line in out {
                let _ = write_port(port, line.as_bytes(), &self.name);
                // Let the adapter put it on the bus before the channel closes.
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = write_port(port, b"C\r", &self.name);
        }
    }
}

fn show_events(snapshot: &Mutex<Snapshot>, job: &Job) {
    let mut snap = lock(snapshot);
    snap.event_rows = job.records().to_vec();
    snap.event_note = job.reading_note().into();
    snap.events_ready = true;
    snap.read_busy = true;
}

fn publish(snapshot: &Mutex<Snapshot>, job: &Job) {
    let mut snap = lock(snapshot);
    match job.ask() {
        Ask::Events => {
            snap.event_rows = job.records().to_vec();
            snap.event_note = job.done_note();
            snap.events_ready = true;
        }
        Ask::Ecu => {
            snap.ecu_values = job.values().to_vec();
            snap.ecu_read_ms = Some(crate::snapshot::now_ms());
            snap.param_note = job.done_note();
        }
        Ask::Battery => {
            snap.battery_values = job.values().to_vec();
            snap.battery_read_ms = Some(crate::snapshot::now_ms());
            snap.param_note = job.done_note();
        }
        Ask::SetClock(_) => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_port(port: &mut dyn std::io::Write, bytes: &[u8], port_name: &str) -> Result<(), String> {
    port.write_all(bytes)
        .map_err(|err| format!("Could not write to {port_name}: {err}"))?;
    port.flush()
        .map_err(|err| format!("Could not write to {port_name}: {err}"))
}

fn push_byte(pending: &mut Vec<u8>, byte: u8) -> Option<Frame> {
    if byte == b'\r' || byte == b'\n' {
        let frame = parse_line(pending);
        pending.clear();
        return frame;
    }
    if byte == 0x07 {
        pending.clear();
        return None;
    }
    if pending.len() < 64 {
        pending.push(byte);
    } else {
        pending.clear();
    }
    None
}

pub fn parse_line(line: &[u8]) -> Option<Frame> {
    let line = std::str::from_utf8(line).ok()?.trim();
    // A real line is plain ASCII. Noise that happens to be valid UTF-8 must
    // not reach the slicing below, which panics inside a multi-byte character.
    if !line.is_ascii() || !line.starts_with('t') || line.len() < 5 {
        return None;
    }
    let id = u16::from_str_radix(&line[1..4], 16).ok()?;
    // Three hex digits reach 0xFFF, but a standard identifier stops at 0x7FF.
    // Anything above is a damaged line, and would add a phantom identifier to
    // the Frames list. (The capture reader already refuses these.)
    if id > 0x7FF {
        return None;
    }
    let dlc = usize::from_str_radix(&line[4..5], 16).ok()?;
    if dlc > 8 || line.len() < 5 + dlc * 2 {
        return None;
    }
    let mut data = [0u8; 8];
    for index in 0..dlc {
        let start = 5 + index * 2;
        data[index] = u8::from_str_radix(&line[start..start + 2], 16).ok()?;
    }
    Some(Frame { id, data, len: dlc })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn ports() -> Vec<String> {
    let mut names: Vec<String> = serialport::available_ports()
        .map(|list| list.into_iter().map(|info| info.port_name).collect())
        .unwrap_or_default();
    names.sort();
    let callouts: Vec<String> = names
        .iter()
        .filter(|name| name.contains("/dev/cu."))
        .cloned()
        .collect();
    names.retain(|name| {
        let Some(suffix) = name.strip_prefix("/dev/tty.") else {
            return true;
        };
        !callouts
            .iter()
            .any(|other| other == &format!("/dev/cu.{suffix}"))
    });
    names
}

/// In a browser the only place to connect is the device that served the page.
#[cfg(target_arch = "wasm32")]
pub fn ports() -> Vec<String> {
    crate::platform::device_url().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn parses_a_standard_slcan_frame() {
        let frame = parse_line(b"t184845820100040A0000").unwrap();
        assert_eq!(frame.id, 0x184);
        assert_eq!(frame.len, 8);
        assert_eq!(
            frame.payload(),
            [0x45, 0x82, 0x01, 0x00, 0x04, 0x0A, 0x00, 0x00]
        );
    }

    #[test]
    fn ignores_extended_and_short_lines() {
        assert!(parse_line(b"T00000184845820100040A0000").is_none());
        assert!(parse_line(b"t181").is_none());
        assert!(parse_line(b"S5").is_none());
    }

    fn feed(session: &mut Session, snapshot: &Mutex<Snapshot>, text: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for byte in text {
            session.on_byte(*byte, snapshot, &mut out);
        }
        out
    }

    #[test]
    fn a_session_decodes_frames_and_asks_for_the_pack_id_once() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let out = feed(&mut session, &snapshot, b"t1818006400003A02F6FF\rt1818006400003A02F6FF\r");
        assert_eq!(out, ["r1838\r"], "the pack id, once");
        let snap = lock(&snapshot);
        assert_eq!(snap.soc, Some(100.0));
        assert_eq!(snap.voltage, Some(57.0));
        assert_eq!(snap.explorer.total, 2);
    }

    fn pack_frame() -> Frame {
        Frame::new(0x181, &[0x18, 0x64, 0, 0, 0x3A, 0x02, 0xF6, 0xFF])
    }

    /// The ECU's status frame: byte 3 bit 0 is "Sending on debug mode".
    fn status_frame(byte3: u8) -> Frame {
        Frame::new(0x28A, &[0, 0, 0, byte3, 0x0C, 0, 0, 0])
    }

    /// A snapshot with the person's debug switch on.
    fn switched_on() -> Mutex<Snapshot> {
        Mutex::new(Snapshot { debug_wanted: true, ..Snapshot::default() })
    }

    fn state(snapshot: &Mutex<Snapshot>) -> DebugState {
        lock(snapshot).debug
    }

    /// A busy bus: a frame every 100 ms from `from` to `to` (milliseconds after
    /// `t0`), with the session checked each time. Returns what it sent.
    fn run_bus(session: &mut Session, snapshot: &Mutex<Snapshot>, t0: Instant, from: u64, to: u64) -> Vec<String> {
        let mut out = Vec::new();
        for ms in (from..=to).step_by(100) {
            let now = t0 + Duration::from_millis(ms);
            session.note_frame(now, &pack_frame(), snapshot);
            session.check_debug(now, snapshot, &mut out);
        }
        out
    }

    #[test]
    fn the_first_frame_asks_for_the_pack_id_and_sends_no_debug_mode_yet() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let out = feed(&mut session, &snapshot, b"t1818006400003A02F6FF\r");
        assert_eq!(out, ["r1838\r"]);
        assert_eq!(state(&snapshot), DebugState::Idle);
    }

    #[test]
    fn debug_mode_is_sent_once_after_a_short_look_and_not_again_while_the_ecu_has_not_answered_yet() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        session.note_frame(t0, &pack_frame(), &snapshot);
        let mut out = Vec::new();
        session.check_debug(at(100), &snapshot, &mut out);
        assert!(out.is_empty(), "still looking at 100 ms");
        session.check_debug(at(200), &snapshot, &mut out);
        assert_eq!(out, [DEBUG_ON_LINE]);
        assert_eq!(state(&snapshot), DebugState::Sent);
        out.clear();
        for ms in [250, 400, 700, 900] {
            session.note_frame(at(ms), &pack_frame(), &snapshot);
            session.check_debug(at(ms), &snapshot, &mut out);
        }
        assert!(out.is_empty(), "no second send inside the confirm wait: {out:?}");
    }

    #[test]
    fn nothing_is_sent_when_the_ecu_already_reports_debug_mode() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        session.note_frame(t0, &pack_frame(), &snapshot);
        session.note_frame(at(2), &status_frame(0x09), &snapshot);
        assert_eq!(state(&snapshot), DebugState::AlreadyOn);
        let mut out = Vec::new();
        for ms in [200, 600, 1500, 5000] {
            session.note_frame(at(ms), &pack_frame(), &snapshot);
            session.check_debug(at(ms), &snapshot, &mut out);
        }
        assert!(out.is_empty(), "not even once: {out:?}");
        // The app did not turn it on, so it does not turn it off.
        session.disconnecting(&snapshot, &mut out);
        assert!(out.is_empty(), "and no debug off: {out:?}");
    }

    #[test]
    fn a_status_frame_without_the_debug_bit_does_not_count() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        session.note_frame(t0, &pack_frame(), &snapshot);
        session.note_frame(t0 + Duration::from_millis(5), &status_frame(0x08), &snapshot);
        assert_eq!(state(&snapshot), DebugState::Idle);
        let mut out = Vec::new();
        session.check_debug(t0 + Duration::from_millis(200), &snapshot, &mut out);
        assert_eq!(out, [DEBUG_ON_LINE], "so it is still sent");
    }

    #[test]
    fn once_the_ecu_confirms_it_is_not_sent_again_and_is_switched_off_at_the_end() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        let first = run_bus(&mut session, &snapshot, t0, 0, 200);
        assert_eq!(first, [DEBUG_ON_LINE]);
        session.note_frame(t0 + Duration::from_millis(260), &status_frame(0x09), &snapshot);
        assert_eq!(state(&snapshot), DebugState::Confirmed);
        let later = run_bus(&mut session, &snapshot, t0, 300, 9000);
        assert!(later.is_empty(), "nothing more in nine seconds: {later:?}");
        let mut out = Vec::new();
        session.disconnecting(&snapshot, &mut out);
        assert_eq!(out, [DEBUG_OFF_LINE]);
        assert_eq!(state(&snapshot), DebugState::Idle);
    }

    #[test]
    fn with_no_confirmation_it_sends_twice_in_all_then_gives_up_and_says_so() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        // The first send, after the short look.
        let mut out = run_bus(&mut session, &snapshot, t0, 0, 1100);
        assert_eq!(out, [DEBUG_ON_LINE], "one send in the first 1.1 s");
        assert_eq!(state(&snapshot), DebugState::Sent);
        // A second, a second after the first, and then it gives up.
        out.extend(run_bus(&mut session, &snapshot, t0, 1200, 2100));
        assert_eq!(out, [DEBUG_ON_LINE, DEBUG_ON_LINE], "the second and last send");
        assert_eq!(state(&snapshot), DebugState::Sent);
        out.extend(run_bus(&mut session, &snapshot, t0, 2200, 20_000));
        assert_eq!(out, [DEBUG_ON_LINE, DEBUG_ON_LINE], "never a third");
        assert_eq!(state(&snapshot), DebugState::Unconfirmed);
        // It was sent, so it is switched off at the end.
        let mut goodbye = Vec::new();
        session.disconnecting(&snapshot, &mut goodbye);
        assert_eq!(goodbye, [DEBUG_OFF_LINE]);
    }

    #[test]
    fn a_sleeping_bus_stops_everything_and_the_next_wake_starts_again() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        session.note_frame(t0, &pack_frame(), &snapshot);
        let mut out = Vec::new();
        session.check_debug(at(200), &snapshot, &mut out);
        assert_eq!(out.len(), 1);
        // The bus goes quiet: nothing is sent to it, and the note clears.
        session.check_debug(at(1500), &snapshot, &mut out);
        session.check_debug(at(3000), &snapshot, &mut out);
        assert_eq!(out.len(), 1, "nothing sent to a sleeping bus");
        assert_eq!(state(&snapshot), DebugState::Idle);
        let mut goodbye = Vec::new();
        session.disconnecting(&snapshot, &mut goodbye);
        assert!(goodbye.is_empty(), "and no debug off to it either");
        // Disconnecting put the switch back to off; the person turns it on again.
        lock(&snapshot).debug_wanted = true;
        // It wakes again: the same look, then one send.
        session.note_frame(at(4000), &pack_frame(), &snapshot);
        session.check_debug(at(4100), &snapshot, &mut out);
        assert_eq!(out.len(), 1, "looking");
        session.check_debug(at(4200), &snapshot, &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn with_the_switch_off_nothing_is_ever_sent() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let t0 = Instant::now();
        let out = run_bus(&mut session, &snapshot, t0, 0, 20_000);
        assert!(out.is_empty(), "listen only: {out:?}");
        assert_eq!(state(&snapshot), DebugState::Idle);
        let mut goodbye = Vec::new();
        session.disconnecting(&snapshot, &mut goodbye);
        assert!(goodbye.is_empty());
        // Even a status frame that says debug mode is on changes nothing.
        session.note_frame(t0 + Duration::from_millis(30_000), &status_frame(0x09), &snapshot);
        assert_eq!(state(&snapshot), DebugState::Idle);
    }

    #[test]
    fn switching_it_on_while_the_scooter_is_awake_starts_a_look_and_one_send() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let t0 = Instant::now();
        assert!(run_bus(&mut session, &snapshot, t0, 0, 500).is_empty());
        lock(&snapshot).debug_wanted = true;
        let out = run_bus(&mut session, &snapshot, t0, 600, 1000);
        assert_eq!(out, [DEBUG_ON_LINE], "one send, after the look: {out:?}");
        assert_eq!(state(&snapshot), DebugState::Sent);
    }

    #[test]
    fn switching_it_off_sends_debug_off_once_if_the_app_had_sent_debug_on() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        let first = run_bus(&mut session, &snapshot, t0, 0, 400);
        assert_eq!(first, [DEBUG_ON_LINE]);
        lock(&snapshot).debug_wanted = false;
        let off = run_bus(&mut session, &snapshot, t0, 500, 3000);
        assert_eq!(off, [DEBUG_OFF_LINE], "once, and nothing more: {off:?}");
        assert_eq!(state(&snapshot), DebugState::Idle);
    }

    #[test]
    fn switching_it_off_when_the_ecu_already_had_it_on_sends_nothing() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        session.note_frame(t0, &pack_frame(), &snapshot);
        session.note_frame(t0 + Duration::from_millis(2), &status_frame(0x09), &snapshot);
        assert_eq!(state(&snapshot), DebugState::AlreadyOn);
        lock(&snapshot).debug_wanted = false;
        let out = run_bus(&mut session, &snapshot, t0, 100, 2000);
        assert!(out.is_empty(), "the app did not turn it on, so it does not turn it off: {out:?}");
        assert_eq!(state(&snapshot), DebugState::Idle);
    }

    #[test]
    fn disconnecting_puts_the_switch_back_to_off() {
        let snapshot = switched_on();
        let mut session = Session::new();
        let t0 = Instant::now();
        let _ = run_bus(&mut session, &snapshot, t0, 0, 400);
        let mut out = Vec::new();
        session.disconnecting(&snapshot, &mut out);
        assert_eq!(out, [DEBUG_OFF_LINE]);
        assert!(!lock(&snapshot).debug_wanted, "the next connection only listens");
    }

    #[test]
    fn nothing_was_heard_so_nothing_is_switched_off() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let mut out = Vec::new();
        session.disconnecting(&snapshot, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn the_only_frames_the_session_sends_on_0x28a_are_the_two_debug_ones() {
        assert_eq!(DEBUG_ON_LINE, "t28A80000000100000000\r");
        assert_eq!(DEBUG_OFF_LINE, "t28A80000000000000000\r");
        let on = parse_line(DEBUG_ON_LINE.as_bytes()).unwrap();
        assert_eq!((on.id, on.len, on.data), (0x28A, 8, [0, 0, 0, 1, 0, 0, 0, 0]));
        let off = parse_line(DEBUG_OFF_LINE.as_bytes()).unwrap();
        assert_eq!((off.id, off.len, off.data), (0x28A, 8, [0; 8]));
    }

    #[test]
    fn a_session_survives_noise_and_split_lines() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        feed(&mut session, &snapshot, b"\x07garbage\r\nt18180064");
        assert_eq!(lock(&snapshot).explorer.total, 0);
        feed(&mut session, &snapshot, b"00003A02F6FF\r");
        assert_eq!(lock(&snapshot).explorer.total, 1);
    }

    #[test]
    fn an_identifier_above_the_eleven_bit_range_is_not_a_frame() {
        // A standard CAN identifier stops at 0x7FF. A damaged line can still
        // carry three hex digits above that.
        assert!(parse_line(b"t7FF0").is_some());
        assert!(parse_line(b"t8000").is_none());
        assert!(parse_line(b"tC0D0").is_none());
        assert!(parse_line(b"tFFF8000000000000000").is_none());
    }

    #[test]
    fn noise_that_is_valid_utf8_is_not_a_frame_and_does_not_panic() {
        assert!(parse_line("t12\u{e9}0".as_bytes()).is_none());
        assert!(parse_line("t1\u{e9}\u{e9}800".as_bytes()).is_none());
        assert!(parse_line("t1818006400003A02F6FF".as_bytes()).is_some());
    }

    /// Starts a read of the given kind and returns the session, the snapshot
    /// and the lines the session wants sent.
    fn start_read(ask: Ask) -> (Session, Mutex<Snapshot>, Vec<String>) {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let (tx, rx) = mpsc::channel();
        let mut out = Vec::new();
        tx.send(ask).unwrap();
        session.poll(&snapshot, &rx, &mut out);
        assert!(lock(&snapshot).read_busy, "the read should have started");
        (session, snapshot, out)
    }

    /// The three frames of the event-log record with this index, as lines.
    fn record_lines(index: u32) -> Vec<u8> {
        let first = [(index as u8 & 0x0F) << 4, (index >> 4) as u8, 26, 10, 5, 11, 37, 0];
        let mut text = String::new();
        for frame in [first, [0, 0, 0x22, 0x10, 0, 0, 0, 0], [0u8; 8]] {
            text.push_str("t1EE8");
            for byte in frame {
                text.push_str(&format!("{byte:02X}"));
            }
            text.push('\r');
        }
        text.into_bytes()
    }

    #[test]
    fn an_event_read_ends_at_record_zero() {
        let (mut session, snapshot, _out) = start_read(Ask::Events);
        feed(&mut session, &snapshot, &record_lines(1));
        assert!(lock(&snapshot).read_busy, "record 1 is not the end");
        feed(&mut session, &snapshot, &record_lines(0));
        let snap = lock(&snapshot);
        assert!(!snap.read_busy, "the read should be over");
        assert_eq!(snap.event_rows.len(), 2);
        assert_eq!(snap.event_rows[0][4], "1", "the record number is kept with the row");
        assert_eq!(snap.event_rows[1][4], "0");
        assert!(snap.event_rows[1][5].contains(" | "), "and so are the raw frames");
        assert_eq!(snap.event_note, "Event log read.");
    }

    #[test]
    fn the_next_page_goes_out_after_a_pause_and_the_pause_is_not_silence() {
        let (mut session, snapshot, mut out) = start_read(Ask::Events);
        assert_eq!(out, ["t1ED0\r"], "the first request");
        for index in (148..=157).rev() {
            feed(&mut session, &snapshot, &record_lines(index));
        }
        assert_eq!(out.len(), 1, "the next page does not go out at once");
        let start = Instant::now();
        session.check_stalled(start + Duration::from_millis(500), &snapshot, &mut out);
        assert_eq!(out.len(), 1, "still inside the pause");
        assert!(lock(&snapshot).read_busy);
        // The pause is longer than the time after which silence ends a read.
        session.check_stalled(start + Duration::from_millis(1100), &snapshot, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], crate::stored::event_page_line(147, 10));
        assert!(lock(&snapshot).read_busy);
        // The page comes, and so does the one after it.
        for index in (138..=147).rev() {
            feed(&mut session, &snapshot, &record_lines(index));
        }
        session.check_stalled(start + Duration::from_millis(2200), &snapshot, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2], crate::stored::event_page_line(137, 10));
        assert_eq!(lock(&snapshot).event_rows.len(), 20);
    }

    #[test]
    fn a_page_that_never_comes_ends_the_read_with_what_it_has() {
        let (mut session, snapshot, mut out) = start_read(Ask::Events);
        for index in (148..=157).rev() {
            feed(&mut session, &snapshot, &record_lines(index));
        }
        let start = Instant::now();
        session.check_stalled(start + Duration::from_millis(1100), &snapshot, &mut out);
        assert_eq!(out.len(), 2, "the next page was asked for");
        // The first look after the page went out notes the replies so far and
        // restarts the quiet time from there.
        session.check_stalled(start + Duration::from_millis(2000), &snapshot, &mut out);
        session.check_stalled(start + Duration::from_millis(3000), &snapshot, &mut out);
        assert!(lock(&snapshot).read_busy, "only one second of waiting for the page");
        session.check_stalled(start + Duration::from_millis(3600), &snapshot, &mut out);
        let snap = lock(&snapshot);
        assert!(!snap.read_busy);
        assert_eq!(snap.event_rows.len(), 10);
        assert_eq!(snap.event_note, "The event log stopped part way. Showing what was read.");
    }

    /// The battery's clock frame (0x200) as a line, for this moment, marked valid.
    fn clock_frame(moment: Moment) -> Vec<u8> {
        let bcd = |value: u8| (value / 10) << 4 | (value % 10);
        format!(
            "t200703{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}\r",
            bcd(moment.hour),
            bcd(moment.minute),
            bcd(moment.second),
            bcd(moment.day),
            bcd(moment.month),
            bcd((moment.year % 100) as u8)
        )
        .into_bytes()
    }

    fn at(hour: u8, minute: u8, second: u8) -> Moment {
        Moment { year: 2026, month: 11, day: 9, hour, minute, second }
    }

    #[test]
    fn setting_the_clock_sends_one_frame_and_ends_when_the_battery_shows_the_new_time() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let (tx, rx) = mpsc::channel();
        let mut out = Vec::new();
        // The clock reads January 2001 to begin with.
        let old = Moment { year: 2001, month: 1, day: 18, hour: 11, minute: 30, second: 0 };
        feed(&mut session, &snapshot, &clock_frame(old));
        out.clear();
        let target = at(22, 28, 3);
        tx.send(Ask::SetClock(target)).unwrap();
        session.poll(&snapshot, &rx, &mut out);
        assert_eq!(out, ["t201704222803091126\r"], "exactly the frame that was sent on 9 Nov 2026");
        assert_eq!(lock(&snapshot).clock_note, clock::NOTE_SETTING);
        assert!(!lock(&snapshot).read_busy, "a clock set is not a read");
        let start = Instant::now();
        // Still the old time: not yet.
        session.check_clock(start + Duration::from_millis(500), &snapshot);
        assert_eq!(lock(&snapshot).clock_note, clock::NOTE_SETTING);
        // Then the battery's frame shows the new time, a second on.
        feed(&mut session, &snapshot, &clock_frame(at(22, 28, 4)));
        session.check_clock(start + Duration::from_millis(1000), &snapshot);
        assert_eq!(lock(&snapshot).clock_note, clock::NOTE_MATCHES);
        // The check is over: a later, wrong clock does not change the note, and nothing more is sent.
        session.check_clock(start + Duration::from_secs(10), &snapshot);
        assert_eq!(lock(&snapshot).clock_note, clock::NOTE_MATCHES);
        session.poll(&snapshot, &rx, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_clock_that_does_not_change_is_said_not_to_match_after_three_seconds() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let (tx, rx) = mpsc::channel();
        let mut out = Vec::new();
        let old = Moment { year: 2001, month: 1, day: 18, hour: 11, minute: 30, second: 0 };
        feed(&mut session, &snapshot, &clock_frame(old));
        tx.send(Ask::SetClock(at(22, 28, 3))).unwrap();
        session.poll(&snapshot, &rx, &mut out);
        let start = Instant::now();
        feed(&mut session, &snapshot, &clock_frame(old));
        session.check_clock(start + Duration::from_millis(2900), &snapshot);
        assert_eq!(lock(&snapshot).clock_note, clock::NOTE_SETTING, "still waiting");
        session.check_clock(start + Duration::from_millis(3100), &snapshot);
        assert_eq!(lock(&snapshot).clock_note, clock::NOTE_DIFFERS);
    }

    #[test]
    fn a_date_that_cannot_be_sent_sends_nothing() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let (tx, rx) = mpsc::channel();
        let mut out = Vec::new();
        tx.send(Ask::SetClock(Moment { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 })).unwrap();
        session.poll(&snapshot, &rx, &mut out);
        assert!(out.is_empty());
        assert_eq!(lock(&snapshot).clock_note, clock::NOTE_NO_TIME);
    }

    #[test]
    fn a_controller_fault_during_the_read_stops_it_and_says_so() {
        let (mut session, snapshot, mut out) = start_read(Ask::Events);
        feed(&mut session, &snapshot, b"t41180000000009001000\r");
        feed(&mut session, &snapshot, &record_lines(40));
        assert!(lock(&snapshot).read_busy);
        feed(&mut session, &snapshot, b"t41180000804E09001000\r");
        {
            let snap = lock(&snapshot);
            assert!(!snap.read_busy, "the read should have stopped");
            assert_eq!(snap.event_rows.len(), 1, "what was read is kept");
            assert_eq!(
                snap.event_note,
                "The motor controller reported a fault, so the read was stopped; switch the scooter off and on again to clear it."
            );
        }
        // Nothing is sent after the fault, even if a page was waiting.
        let before = out.len();
        session.check_stalled(Instant::now() + Duration::from_millis(5000), &snapshot, &mut out);
        assert_eq!(out.len(), before);
    }

    #[test]
    fn a_read_that_has_ended_lets_the_next_one_start() {
        let (mut session, snapshot, mut out) = start_read(Ask::Events);
        feed(&mut session, &snapshot, b"t1EE80000000000000000\r");
        let start = Instant::now();
        session.check_stalled(start + Duration::from_millis(100), &snapshot, &mut out);
        session.check_stalled(start + Duration::from_millis(2000), &snapshot, &mut out);
        assert!(!lock(&snapshot).read_busy);
        let (tx, rx) = mpsc::channel();
        tx.send(Ask::Battery).unwrap();
        session.poll(&snapshot, &rx, &mut out);
        assert!(lock(&snapshot).read_busy, "a new read starts after the old one ended");
        assert_eq!(out.last().map(String::as_str), Some("t234101\r"));
    }

    #[test]
    fn an_event_read_that_gets_no_answer_gives_up() {
        let (mut session, snapshot, mut out) = start_read(Ask::Events);
        let start = Instant::now();
        session.check_stalled(start + Duration::from_millis(2000), &snapshot, &mut out);
        assert!(lock(&snapshot).read_busy, "still waiting for a first reply");
        session.check_stalled(start + Duration::from_millis(3500), &snapshot, &mut out);
        let snap = lock(&snapshot);
        assert!(!snap.read_busy);
        assert_eq!(snap.event_note, "The event log did not answer.");
    }

    #[test]
    fn a_parameter_read_asks_again_twice_and_then_gives_up() {
        let (mut session, snapshot, mut out) = start_read(Ask::Ecu);
        assert_eq!(out.len(), 1, "the opening request");
        let start = Instant::now();
        session.check_stalled(start + Duration::from_millis(2100), &snapshot, &mut out);
        assert_eq!(out.len(), 2, "first retry");
        assert_eq!(out[1], out[0], "the same request again");
        session.check_stalled(start + Duration::from_millis(4300), &snapshot, &mut out);
        assert_eq!(out.len(), 3, "second retry");
        assert!(lock(&snapshot).read_busy);
        session.check_stalled(start + Duration::from_millis(6500), &snapshot, &mut out);
        assert_eq!(out.len(), 3, "no third retry");
        let snap = lock(&snapshot);
        assert!(!snap.read_busy);
        assert_eq!(snap.param_note, "The ECU stopped answering. Showing what was read.");
    }

    #[test]
    fn a_reply_restarts_the_wait() {
        let (mut session, snapshot, mut out) = start_read(Ask::Battery);
        let start = Instant::now();
        // The reply to the first request (index 1) arrives; the read moves on.
        let after_reply = feed(&mut session, &snapshot, b"t235801409C0000000000\r");
        assert!(
            after_reply.iter().any(|line| line == "t234102\r"),
            "the next request should have been sent: {after_reply:?}"
        );
        // The reply was just heard, so the wait starts again from here.
        session.check_stalled(start + Duration::from_millis(100), &snapshot, &mut out);
        let sent = out.len();
        session.check_stalled(start + Duration::from_millis(1900), &snapshot, &mut out);
        assert_eq!(out.len(), sent, "no retry while replies are still arriving");
        session.check_stalled(start + Duration::from_millis(2300), &snapshot, &mut out);
        assert_eq!(out.len(), sent + 1, "a retry once the wait has passed");
    }

    #[test]
    fn a_requested_read_starts_on_the_next_poll() {
        let snapshot = Mutex::new(Snapshot::default());
        let mut session = Session::new();
        let (tx, rx) = mpsc::channel();
        let mut out = Vec::new();
        session.poll(&snapshot, &rx, &mut out);
        assert!(out.is_empty());
        tx.send(Ask::Ecu).unwrap();
        session.poll(&snapshot, &rx, &mut out);
        assert!(lock(&snapshot).read_busy);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].ends_with('\r'));
    }
}
