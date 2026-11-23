//! The SD card on the unit (an ESP32 with an Atomic TF Card Base): what the
//! unit's card endpoints say, and how the app asks them.
//!
//! This is only for a page served by the unit itself, in a browser. A computer
//! with a CANable has no such card, so there `available` never becomes true and
//! the SD card screen never appears.
//!
//! Everything the unit sends is plain text, `key value` lines for the status and
//! tab-separated lines for a folder, so no parser library is needed. The unit
//! is the one that decides what a path may be; the checks here only save a
//! round trip and say why in words.

// Parts of this are used only by the browser build (the file helpers and the
// requests the page sends itself).
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::sync::Mutex;

// ---- Requests and answers ----------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Info,
    Time,
    Status,
    List,
    Mkdir,
    Delete,
    Format,
    Eject,
    Mount,
    Record,
    Auto,
    Mark,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub kind: Kind,
    pub method: &'static str,
    pub url: String,
    /// What it was about (a path), handed back with the answer.
    pub tag: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub kind: Kind,
    pub tag: String,
    /// The HTTP status, or 0 when the unit did not answer at all.
    pub status: u16,
    pub text: String,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// What went wrong, in words, for a person.
    pub fn problem(&self) -> String {
        let text = self.text.trim();
        if self.status == 0 {
            if text.is_empty() {
                "The unit did not answer.".to_string()
            } else {
                text.to_string()
            }
        } else if text.is_empty() {
            format!("The unit answered {}.", self.status)
        } else {
            capitalise(text)
        }
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str().trim_end_matches('.')),
        None => String::new(),
    }
}

/// Percent-encodes everything but letters, digits and `-_.~`.
pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn get(kind: Kind, url: String, tag: &str) -> Request {
    Request { kind, method: "GET", url, tag: tag.to_string() }
}

fn post(kind: Kind, url: String, tag: &str) -> Request {
    Request { kind, method: "POST", url, tag: tag.to_string() }
}

pub fn info() -> Request {
    get(Kind::Info, "/info".into(), "")
}

/// Tells the unit the time, in seconds, as this device's own clock shows it.
pub fn time(local_seconds: i64) -> Request {
    post(Kind::Time, format!("/time?local={local_seconds}"), "")
}

pub fn status() -> Request {
    get(Kind::Status, "/sd/status".into(), "")
}

pub fn list(path: &str) -> Request {
    get(Kind::List, format!("/sd/list?path={}", encode(path)), path)
}

pub fn mkdir(path: &str) -> Request {
    post(Kind::Mkdir, format!("/sd/mkdir?path={}", encode(path)), path)
}

pub fn delete(path: &str) -> Request {
    post(Kind::Delete, format!("/sd/delete?path={}", encode(path)), path)
}

pub fn format() -> Request {
    post(Kind::Format, "/sd/format".into(), "")
}

pub fn eject() -> Request {
    post(Kind::Eject, "/sd/eject".into(), "")
}

pub fn mount() -> Request {
    post(Kind::Mount, "/sd/mount".into(), "")
}

pub fn record(on: bool) -> Request {
    post(Kind::Record, format!("/sd/record?on={}", u8::from(on)), "")
}

pub fn auto(on: bool) -> Request {
    post(Kind::Auto, format!("/sd/auto?on={}", u8::from(on)), "")
}

/// A marker for the unit's recording. The unit takes at most 63 bytes.
pub fn mark(text: &str) -> Request {
    let mut cut = String::new();
    for c in text.trim().chars() {
        if cut.len() + c.len_utf8() > 60 {
            break;
        }
        cut.push(c);
    }
    post(Kind::Mark, format!("/sd/mark?text={}", encode(&cut)), "")
}

pub fn file_url(path: &str) -> String {
    format!("/sd/file?path={}", encode(path))
}

// ---- What the unit says ------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub version: String,
    pub hardware: String,
    pub sd: bool,
}

pub fn parse_info(text: &str) -> Info {
    let mut info = Info::default();
    for line in text.lines() {
        match line.split_once(' ') {
            Some(("version", value)) => info.version = value.trim().to_string(),
            Some(("hardware", value)) => info.hardware = value.trim().to_string(),
            Some(("sd", value)) => info.sd = value.trim() == "1",
            _ => {}
        }
    }
    info
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CardState {
    /// Not asked yet, or the answer was not understood.
    #[default]
    Unknown,
    NoCard,
    NoFilesystem,
    TooBig,
    Ejected,
    Ready,
    Formatting,
    Unmounting,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub state: CardState,
    pub message: String,
    pub card_name: String,
    pub card_type: String,
    pub card_bytes: u64,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub space_known: bool,
    /// The job the unit is busy with (`download`, `upload`, `delete`, `format`, `eject`), if any.
    pub busy: Option<String>,
    pub busy_done: u64,
    pub busy_total: u64,
    pub recording: bool,
    pub record_wanted: bool,
    pub record_file: String,
    pub record_frames: u64,
    pub record_bytes: u64,
    pub record_lost: u64,
    pub auto: bool,
    pub clock: bool,
    pub heap_free: u64,
}

pub fn parse_status(text: &str) -> Status {
    let mut s = Status::default();
    for line in text.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let number = || value.trim().parse::<u64>().unwrap_or(0);
        let flag = || value.trim() == "1";
        match key {
            "state" => {
                s.state = match value.trim() {
                    "nocard" => CardState::NoCard,
                    "nofs" => CardState::NoFilesystem,
                    "toobig" => CardState::TooBig,
                    "ejected" => CardState::Ejected,
                    "ready" => CardState::Ready,
                    "formatting" => CardState::Formatting,
                    "unmounting" => CardState::Unmounting,
                    _ => CardState::Unknown,
                }
            }
            "message" => s.message = value.trim().to_string(),
            "card_name" => s.card_name = value.trim().to_string(),
            "card_type" => s.card_type = value.trim().to_string(),
            "card_bytes" => s.card_bytes = number(),
            "total_bytes" => s.total_bytes = number(),
            "free_bytes" => s.free_bytes = number(),
            "space_known" => s.space_known = flag(),
            "busy" => s.busy = Some(value.trim().to_string()).filter(|job| !job.is_empty() && job != "none"),
            "busy_done" => s.busy_done = number(),
            "busy_total" => s.busy_total = number(),
            "recording" => s.recording = flag(),
            "record_wanted" => s.record_wanted = flag(),
            "record_file" => s.record_file = value.trim().to_string(),
            "record_frames" => s.record_frames = number(),
            "record_bytes" => s.record_bytes = number(),
            "record_lost" => s.record_lost = number(),
            "auto" => s.auto = flag(),
            "clock" => s.clock = flag(),
            "heap_free" => s.heap_free = number(),
            _ => {}
        }
    }
    s
}

impl Status {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub dir: bool,
    pub name: String,
    pub size: u64,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
    /// The folder holds more than the unit sent.
    pub more: bool,
}

/// Folders first, then by name, ignoring case.
pub fn parse_listing(text: &str) -> Listing {
    let mut listing = Listing::default();
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("path ") {
            listing.path = path.to_string();
        } else if line == "more 1" {
            listing.more = true;
        } else {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 && (parts[0] == "d" || parts[0] == "f") && !parts[1].is_empty() {
                listing.entries.push(Entry {
                    dir: parts[0] == "d",
                    name: parts[1].to_string(),
                    size: parts[2].parse().unwrap_or(0),
                    date: parts.get(3).map(|date| date.to_string()).unwrap_or_default(),
                });
            }
        }
    }
    listing
        .entries
        .sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    listing
}

// ---- Paths and sizes ---------------------------------------------------------

/// `folder` and `name` joined, the way the unit writes a path.
pub fn join(folder: &str, name: &str) -> String {
    if folder == "/" {
        format!("/{name}")
    } else {
        format!("{folder}/{name}")
    }
}

/// The folders that lead to `path`, each with the path to open it: for
/// "/a/b" that is ("a", "/a") and ("b", "/a/b").
pub fn crumbs(path: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut so_far = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        so_far.push('/');
        so_far.push_str(part);
        out.push((part.to_string(), so_far.clone()));
    }
    out
}

/// Why a file or folder name would be refused, or `Ok`. The unit makes the same
/// checks (see `firmware/main/sd_paths.h`); this says so before the trip.
pub fn check_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Type a name first.");
    }
    if name.len() > 100 {
        return Err("A name can be at most 100 characters.");
    }
    if name == "." || name == ".." || name.starts_with(' ') || name.ends_with(' ') || name.ends_with('.') {
        return Err("A name cannot start or end with a space, end with a dot, or be only dots.");
    }
    if name.chars().any(|c| !(' '..='~').contains(&c) || "\\/:*?\"<>|".contains(c)) {
        return Err("Use plain letters, digits, spaces and . _ - ( ) only, with none of \\ / : * ? \" < > |.");
    }
    Ok(())
}

/// A size in bytes for a person: "812 B", "34 KB", "1.5 MB", "14.8 GB". Decimal
/// units, the way cards are sold.
pub fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{} KB", (b / 1e3).round() as u64)
    } else if bytes < 1_000_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.1} GB", b / 1e9)
    }
}

// ---- An upload in progress (the page does the sending) -------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Upload {
    #[default]
    Idle,
    Sending {
        name: String,
        done: u64,
        total: u64,
    },
    Finished {
        name: String,
    },
    Failed {
        name: String,
        why: String,
    },
}

/// The page keeps this as one text: `idle`, `sending|name|done|total`,
/// `finished|name` or `failed|name|why`.
pub fn parse_upload(text: &str) -> Upload {
    let parts: Vec<&str> = text.split('|').collect();
    match parts.as_slice() {
        ["sending", name, done, total] => Upload::Sending {
            name: name.to_string(),
            done: done.parse().unwrap_or(0),
            total: total.parse().unwrap_or(0),
        },
        ["finished", name] => Upload::Finished { name: name.to_string() },
        ["failed", name, why @ ..] => Upload::Failed { name: name.to_string(), why: why.join("|") },
        _ => Upload::Idle,
    }
}

// ---- A recording fetched from the card to replay it ---------------------------

/// The page keeps a whole recording in memory to replay it, and a phone has
/// little to spare (the file is held a few times over while it plays), so a
/// larger one is refused with a reason. A first guess, not a measured limit.
pub const REPLAY_MAX_BYTES: u64 = 32_000_000;

/// The file types the replay can read (see `formats.rs`).
const REPLAY_EXTENSIONS: [&str; 4] = ["log", "txt", "csv", "asc"];

/// Why a recording of this size cannot be replayed in the page, if it cannot.
pub fn replay_problem(size: u64) -> Option<String> {
    (size > REPLAY_MAX_BYTES).then(|| {
        format!(
            "Too large to replay in the page ({}; the limit is {}). Download it and replay it on a computer.",
            size_text(size),
            size_text(REPLAY_MAX_BYTES)
        )
    })
}

/// A file the replay could read, judged by its name.
pub fn is_recording_name(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && REPLAY_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Fetch {
    #[default]
    Idle,
    Loading {
        name: String,
        done: u64,
        total: u64,
    },
    Ready {
        name: String,
    },
    Failed {
        name: String,
        why: String,
    },
}

/// The page keeps this as one text: `idle`, `loading|name|done|total`,
/// `ready|name` or `failed|name|why`.
pub fn parse_fetch(text: &str) -> Fetch {
    let parts: Vec<&str> = text.split('|').collect();
    match parts.as_slice() {
        ["loading", name, done, total] => Fetch::Loading {
            name: name.to_string(),
            done: done.parse().unwrap_or(0),
            total: total.parse().unwrap_or(0),
        },
        ["ready", name] => Fetch::Ready { name: name.to_string() },
        ["failed", name, why @ ..] => Fetch::Failed { name: name.to_string(), why: why.join("|") },
        _ => Fetch::Idle,
    }
}

// ---- The mailbox -------------------------------------------------------------

static ANSWERS: Mutex<Vec<Answer>> = Mutex::new(Vec::new());

fn post_answer(answer: Answer) {
    ANSWERS.lock().unwrap_or_else(|poison| poison.into_inner()).push(answer);
}

/// What has come back since the last call, oldest first.
pub fn take_answers() -> Vec<Answer> {
    std::mem::take(&mut *ANSWERS.lock().unwrap_or_else(|poison| poison.into_inner()))
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use super::{post_answer, Answer, Fetch, Request, Upload};

    pub fn send(request: Request) {
        post_answer(Answer {
            kind: request.kind,
            tag: request.tag,
            status: 0,
            text: "There is no SD card here: this only works in the page the unit serves.".to_string(),
        });
    }

    pub fn download(_path: &str) {}

    pub fn pick_upload(_folder: &str) {}

    pub fn upload_state() -> Upload {
        Upload::Idle
    }

    pub fn clear_upload() {}

    pub fn fetch_start(_path: &str, _total: u64) {}

    pub fn fetch_state() -> Fetch {
        Fetch::Idle
    }

    pub fn fetch_take() -> Option<Vec<u8>> {
        None
    }

    pub fn fetch_reset() {}

    pub fn local_seconds() -> Option<i64> {
        None
    }
}

#[cfg(target_arch = "wasm32")]
mod backend {
    use super::{parse_fetch, parse_upload, post_answer, Answer, Fetch, Request, Upload, REPLAY_MAX_BYTES};
    use eframe::wasm_bindgen::{JsCast, JsValue};
    use js_sys::{Function, Reflect};
    use wasm_bindgen_futures::JsFuture;

    pub fn send(request: Request) {
        wasm_bindgen_futures::spawn_local(async move {
            let (status, text) = fetch(&request).await;
            post_answer(Answer { kind: request.kind, tag: request.tag, status, text });
        });
    }

    async fn fetch(request: &Request) -> (u16, String) {
        let Some(window) = web_sys::window() else {
            return (0, "There is no browser window.".to_string());
        };
        let init = web_sys::RequestInit::new();
        init.set_method(request.method);
        init.set_cache(web_sys::RequestCache::NoStore);
        let answer = match JsFuture::from(window.fetch_with_str_and_init(&request.url, &init)).await {
            Ok(answer) => answer,
            Err(_) => return (0, "The unit did not answer.".to_string()),
        };
        let Ok(response) = answer.dyn_into::<web_sys::Response>() else {
            return (0, "The unit sent something unexpected.".to_string());
        };
        let status = response.status();
        let text = match response.text() {
            Ok(promise) => JsFuture::from(promise).await.ok().and_then(|value| value.as_string()).unwrap_or_default(),
            Err(_) => String::new(),
        };
        (status, text)
    }

    /// Starts the browser's own download of a file on the card, so nothing goes
    /// through the app's memory.
    pub fn download(path: &str) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Ok(element) = document.create_element("a") else {
            return;
        };
        let Ok(anchor) = element.dyn_into::<web_sys::HtmlAnchorElement>() else {
            return;
        };
        anchor.set_href(&super::file_url(path));
        anchor.set_download(super::leaf(path));
        anchor.click();
    }

    fn helper(name: &str) -> Option<(JsValue, Function)> {
        let window = web_sys::window()?;
        let store = Reflect::get(&window, &JsValue::from_str("s01Sd")).ok()?;
        let function = Reflect::get(&store, &JsValue::from_str(name)).ok()?.dyn_into::<Function>().ok()?;
        Some((store, function))
    }

    /// Opens the browser's file chooser and sends the chosen file to `folder`.
    pub fn pick_upload(folder: &str) {
        if let Some((store, function)) = helper("pick") {
            let _ = function.call1(&store, &JsValue::from_str(folder));
        }
    }

    pub fn upload_state() -> Upload {
        helper("state")
            .and_then(|(store, function)| function.call0(&store).ok())
            .and_then(|value| value.as_string())
            .map(|text| parse_upload(&text))
            .unwrap_or_default()
    }

    pub fn clear_upload() {
        if let Some((store, function)) = helper("clear") {
            let _ = function.call0(&store);
        }
    }

    /// Starts the page fetching a file from the card into memory, to replay it.
    /// `total` is the size the folder listing gave (the unit sends the file in
    /// pieces and does not say its length).
    pub fn fetch_start(path: &str, total: u64) {
        if let Some((store, function)) = helper("fetchStart") {
            let _ = function.call3(
                &store,
                &JsValue::from_str(path),
                &JsValue::from_f64(total as f64),
                &JsValue::from_f64(REPLAY_MAX_BYTES as f64),
            );
        }
    }

    pub fn fetch_state() -> Fetch {
        helper("fetchState")
            .and_then(|(store, function)| function.call0(&store).ok())
            .and_then(|value| value.as_string())
            .map(|text| parse_fetch(&text))
            .unwrap_or_default()
    }

    /// The bytes of a finished fetch, once; the page forgets them.
    pub fn fetch_take() -> Option<Vec<u8>> {
        let (store, function) = helper("fetchTake")?;
        let value = function.call0(&store).ok()?;
        value.dyn_into::<js_sys::Uint8Array>().ok().map(|array| array.to_vec())
    }

    /// Stops a fetch that is running and forgets its state.
    pub fn fetch_reset() {
        if let Some((store, function)) = helper("fetchReset") {
            let _ = function.call0(&store);
        }
    }

    /// This device's local time as seconds since 1970, taken as if it were UTC.
    pub fn local_seconds() -> Option<i64> {
        let now = js_sys::Date::new_0();
        let seconds = (now.get_time() / 1000.0).floor() as i64;
        let offset_minutes = now.get_timezone_offset() as i64;
        Some(seconds - offset_minutes * 60)
    }
}

/// The last part of a path.
pub fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub use backend::{
    clear_upload, download, fetch_reset, fetch_start, fetch_state, fetch_take, local_seconds, pick_upload, send, upload_state,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fetch_state_text_is_read_back() {
        assert_eq!(parse_fetch("idle"), Fetch::Idle);
        assert_eq!(parse_fetch("nonsense"), Fetch::Idle);
        assert_eq!(parse_fetch("loading|a.log|10|200"), Fetch::Loading { name: "a.log".into(), done: 10, total: 200 });
        assert_eq!(parse_fetch("ready|a.log"), Fetch::Ready { name: "a.log".into() });
        assert_eq!(
            parse_fetch("failed|a.log|this file is being recorded; stop the recording first"),
            Fetch::Failed { name: "a.log".into(), why: "this file is being recorded; stop the recording first".into() }
        );
        assert_eq!(parse_fetch("failed|a.log|x|y"), Fetch::Failed { name: "a.log".into(), why: "x|y".into() });
    }

    #[test]
    fn only_recording_files_of_a_sensible_size_get_a_replay_button() {
        for name in ["rec-1.log", "A.LOG", "x.csv", "notes.txt", "bus.asc"] {
            assert!(is_recording_name(name), "{name}");
        }
        for name in ["firmware.bin", "log", ".log", "photo.jpg", "a.log.bak", "dir"] {
            assert!(!is_recording_name(name), "{name}");
        }
        assert!(replay_problem(REPLAY_MAX_BYTES).is_none());
        let why = replay_problem(40_000_000).expect("too large");
        assert!(why.contains("Too large") && why.contains("40.0 MB") && why.contains("32.0 MB") && why.contains("computer"), "{why}");
    }

    #[test]
    fn info_says_whether_there_is_a_card() {
        let info = parse_info("version 0.13.0\nhardware M5 ATOM Lite + Unit CAN (Grove) + TF Card Base\nsd 1\n");
        assert_eq!(info.version, "0.13.0");
        assert!(info.hardware.ends_with("TF Card Base"));
        assert!(info.sd);
        assert!(!parse_info("version 0.13.0\nhardware x\nsd 0\n").sd);
        assert!(!parse_info("").sd);
        assert!(!parse_info("<html>not the unit</html>").sd);
    }

    #[test]
    fn a_status_is_read_field_by_field() {
        let text = "state ready\nmessage \ncard_name SD16G\ncard_type SDHC\ncard_bytes 15931539456\ntotal_bytes 15900000000\n\
                    free_bytes 14000000000\nspace_known 1\nbusy none\nbusy_done 0\nbusy_total 0\nrecording 1\nrecord_wanted 1\n\
                    record_file /recordings/rec-20261119-214501.log\nrecord_frames 1200\nrecord_bytes 70000\nrecord_lost 3\nauto 1\nclock 1\nheap_free 90000\n";
        let s = parse_status(text);
        assert_eq!(s.state, CardState::Ready);
        assert_eq!(s.card_name, "SD16G");
        assert_eq!(s.total_bytes, 15_900_000_000);
        assert_eq!(s.used_bytes(), 1_900_000_000);
        assert!(s.space_known && s.recording && s.record_wanted && s.auto && s.clock);
        assert_eq!(s.busy, None);
        assert_eq!(s.record_frames, 1200);
        assert_eq!(s.record_lost, 3);
        assert_eq!(s.record_file, "/recordings/rec-20261119-214501.log");
    }

    #[test]
    fn a_busy_unit_names_its_job_and_nonsense_is_harmless() {
        let s = parse_status("state ready\nbusy upload\nbusy_done 4096\nbusy_total 10000\n");
        assert_eq!(s.busy.as_deref(), Some("upload"));
        assert_eq!((s.busy_done, s.busy_total), (4096, 10_000));
        let odd = parse_status("state banana\nfree_bytes lots\nwhat\n\n   \n");
        assert_eq!(odd.state, CardState::Unknown);
        assert_eq!(odd.free_bytes, 0);
        assert_eq!(parse_status("state nofs\n").state, CardState::NoFilesystem);
        assert_eq!(parse_status("state toobig\n").state, CardState::TooBig);
        assert_eq!(parse_status("state ejected\n").state, CardState::Ejected);
    }

    #[test]
    fn a_folder_lists_folders_first_then_by_name() {
        let text = "path /\nf\tzeta.txt\t12\t2026-11-19 21:45:01\nd\trecordings\t0\t\nf\tAlpha.log\t4096\t\nd\tAbc\t0\t2026-11-01 10:00:00\nmore 1\n";
        let l = parse_listing(text);
        assert_eq!(l.path, "/");
        assert!(l.more);
        let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Abc", "recordings", "Alpha.log", "zeta.txt"]);
        assert!(l.entries[0].dir && !l.entries[2].dir);
        assert_eq!(l.entries[2].size, 4096);
        assert_eq!(l.entries[3].date, "2026-11-19 21:45:01");
    }

    #[test]
    fn damaged_listing_lines_are_skipped() {
        let l = parse_listing("path /a\nx\tbad\t1\nf\t\t5\nf\tgood\tnot-a-number\nd\n");
        assert_eq!(l.entries.len(), 1);
        assert_eq!(l.entries[0].name, "good");
        assert_eq!(l.entries[0].size, 0);
        assert!(!l.more);
    }

    #[test]
    fn paths_join_split_and_lead_back() {
        assert_eq!(join("/", "a.txt"), "/a.txt");
        assert_eq!(join("/x", "a.txt"), "/x/a.txt");
        assert_eq!(leaf("/x/a.txt"), "a.txt");
        assert_eq!(crumbs("/"), Vec::<(String, String)>::new());
        assert_eq!(
            crumbs("/a/b"),
            vec![("a".to_string(), "/a".to_string()), ("b".to_string(), "/a/b".to_string())]
        );
    }

    #[test]
    fn names_are_checked_the_way_the_unit_checks_them() {
        assert!(check_name("rec 1.log").is_ok());
        assert!(check_name("A-b_c (2).txt").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name(&"a".repeat(101)).is_err());
        assert!(check_name(&"a".repeat(100)).is_ok());
        for bad in [".", "..", " a", "a ", "a.", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "café", "tab\t"] {
            assert!(check_name(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn urls_are_encoded() {
        assert_eq!(encode("/a b/c.txt"), "%2Fa%20b%2Fc.txt");
        assert_eq!(encode("A-z_0.9~"), "A-z_0.9~");
        assert_eq!(encode("é"), "%C3%A9");
        assert_eq!(list("/rec ordings").url, "/sd/list?path=%2Frec%20ordings");
        assert_eq!(delete("/a").method, "POST");
        assert_eq!(file_url("/a b"), "/sd/file?path=%2Fa%20b");
        assert_eq!(record(true).url, "/sd/record?on=1");
        assert_eq!(auto(false).url, "/sd/auto?on=0");
        assert_eq!(mark("pressed reverse").url, "/sd/mark?text=pressed%20reverse");
        assert!(mark(&"x".repeat(100)).url.len() <= "/sd/mark?text=".len() + 60);
        assert_eq!(mark("  padded ").url, "/sd/mark?text=padded");
        assert_eq!(time(1_795_124_701).url, "/time?local=1795124701");
        assert_eq!(list("/x").tag, "/x");
    }

    #[test]
    fn sizes_read_the_way_a_card_is_sold() {
        assert_eq!(size_text(0), "0 B");
        assert_eq!(size_text(999), "999 B");
        assert_eq!(size_text(1000), "1 KB");
        assert_eq!(size_text(34_500), "35 KB");
        assert_eq!(size_text(1_500_000), "1.5 MB");
        assert_eq!(size_text(15_900_000_000), "15.9 GB");
    }

    #[test]
    fn an_upload_state_from_the_page_is_read() {
        assert_eq!(parse_upload("idle"), Upload::Idle);
        assert_eq!(parse_upload(""), Upload::Idle);
        assert_eq!(
            parse_upload("sending|a.bin|512|2048"),
            Upload::Sending { name: "a.bin".into(), done: 512, total: 2048 }
        );
        assert_eq!(parse_upload("finished|a.bin"), Upload::Finished { name: "a.bin".into() });
        assert_eq!(
            parse_upload("failed|a.bin|the card is full"),
            Upload::Failed { name: "a.bin".into(), why: "the card is full".into() }
        );
        assert_eq!(
            parse_upload("failed|a.bin|a|b"),
            Upload::Failed { name: "a.bin".into(), why: "a|b".into() }
        );
    }

    #[test]
    fn a_problem_is_put_in_words() {
        let answer = |status, text: &str| Answer { kind: Kind::Delete, tag: String::new(), status, text: text.into() };
        assert_eq!(answer(409, "busy with another job").problem(), "Busy with another job.");
        assert_eq!(answer(500, "").problem(), "The unit answered 500.");
        assert_eq!(answer(0, "").problem(), "The unit did not answer.");
        assert!(answer(204, "").ok());
        assert!(!answer(409, "x").ok());
    }
}
