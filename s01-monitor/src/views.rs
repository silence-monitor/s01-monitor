//! The custom views a person saves on the Live and Readings screens.
//!
//! The list is kept as a small text file. On a computer it is a file in the
//! user's settings folder. In a browser it lives on the device that served the
//! page (`GET` and `PUT` on `/views`), so every browser that opens the page
//! sees the same views.
//!
//! Every add or delete reads the stored list first, changes that, and writes it
//! back, so two browsers do not overwrite each other with an out-of-date list.
//! The answer comes back through `take_outcomes`, because in a browser the
//! round trip takes a moment.

use std::sync::Mutex;

use crate::signals::{SignalDef, MAX_SIGNALS};

/// The most bytes the list may take. The device keeps it in flash next to the
/// copy it replaces, so the limit is the same everywhere.
pub const MAX_BYTES: usize = 6000;
const MAX_NAME_CHARS: usize = 40;
const HEADER: &str = "S01 views 1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedView {
    pub id: String,
    pub name: String,
    pub keys: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Live,
    Reading,
}

impl Group {
    fn tag(self) -> &'static str {
        match self {
            Group::Live => "live",
            Group::Reading => "reading",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "live" => Some(Group::Live),
            "reading" => Some(Group::Reading),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saved {
    pub live: Vec<NamedView>,
    pub reading: Vec<NamedView>,
    /// Signals made on the Explore screen. They are kept in the same file as
    /// extra lines, which an older version of the app skips.
    pub signals: Vec<SignalDef>,
}

impl Saved {
    pub fn list(&self, group: Group) -> &Vec<NamedView> {
        match group {
            Group::Live => &self.live,
            Group::Reading => &self.reading,
        }
    }

    fn list_mut(&mut self, group: Group) -> &mut Vec<NamedView> {
        match group {
            Group::Live => &mut self.live,
            Group::Reading => &mut self.reading,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// `reserved` holds the names of the built-in views, which a saved view
    /// may not reuse.
    Add {
        group: Group,
        name: String,
        keys: Vec<String>,
        reserved: Vec<String>,
    },
    Remove {
        group: Group,
        id: String,
    },
    /// The key of a new signal is chosen when it is saved.
    AddSignal(SignalDef),
    RemoveSignal {
        key: String,
    },
}

/// A name with control characters turned into spaces, trimmed and cut short.
pub fn clean_name(name: &str) -> String {
    let spaced: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.trim().chars().take(MAX_NAME_CHARS).collect::<String>().trim().to_string()
}

fn clean_keys(keys: &[String]) -> Vec<String> {
    let mut clean: Vec<String> = Vec::new();
    for key in keys {
        let key = key.trim();
        if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric()) && !clean.iter().any(|k| k == key) {
            clean.push(key.to_string());
        }
    }
    clean
}

/// The number in an id such as `live-3`, when the id belongs to the group.
fn id_number(id: &str, group: Group) -> Option<u32> {
    id.strip_prefix(group.tag())?.strip_prefix('-')?.parse().ok()
}

/// The number in a signal key such as `signal-3`.
fn signal_number(key: &str) -> Option<u32> {
    key.strip_prefix("signal-")?.parse().ok()
}

fn encode_signal(signal: &SignalDef) -> String {
    format!(
        "signal\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        signal.key,
        signal.name,
        signal.id,
        signal.start,
        signal.count,
        u8::from(signal.signed),
        u8::from(signal.big_endian),
        signal.scale,
        signal.offset,
        signal.unit
    )
}

/// A `signal` line, or `None` when anything in it is wrong.
fn decode_signal(parts: &[&str]) -> Option<SignalDef> {
    let [key, name, id, start, count, signed, big_endian, scale, offset, unit] = parts else {
        return None;
    };
    let mut signal = SignalDef {
        key: key.trim().to_string(),
        name: name.to_string(),
        id: id.trim().parse().ok()?,
        start: start.trim().parse().ok()?,
        count: count.trim().parse().ok()?,
        signed: *signed == "1",
        big_endian: *big_endian == "1",
        scale: scale.trim().parse().ok()?,
        offset: offset.trim().parse().ok()?,
        unit: unit.to_string(),
    };
    signal.tidy();
    signal_number(&signal.key)?;
    signal.check().ok()?;
    Some(signal)
}

pub fn encode(saved: &Saved) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for group in [Group::Live, Group::Reading] {
        for view in saved.list(group) {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                group.tag(),
                view.id,
                view.name,
                view.keys.join(",")
            ));
        }
    }
    for signal in &saved.signals {
        out.push_str(&encode_signal(signal));
    }
    out
}

/// Reads a stored list. Anything it does not understand is skipped, so a
/// damaged line costs one view and never the rest.
pub fn decode(text: &str) -> Saved {
    let mut saved = Saved::default();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return saved;
    }
    for line in lines {
        if let Some(rest) = line.strip_prefix("signal\t") {
            let parts: Vec<&str> = rest.split('\t').collect();
            if let Some(signal) = decode_signal(&parts) {
                let clash = saved.signals.iter().any(|kept| kept.key == signal.key || kept.name == signal.name);
                if !clash && saved.signals.len() < MAX_SIGNALS {
                    saved.signals.push(signal);
                }
            }
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(tag), Some(id), Some(name), Some(keys)) = (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Some(group) = Group::from_tag(tag) else {
            continue;
        };
        let id = id.trim();
        let name = clean_name(name);
        let keys = clean_keys(&keys.split(',').map(str::to_string).collect::<Vec<_>>());
        if id_number(id, group).is_none() || name.is_empty() || keys.is_empty() {
            continue;
        }
        let list = saved.list_mut(group);
        if list.iter().any(|view| view.id == id || view.name == name) {
            continue;
        }
        list.push(NamedView {
            id: id.to_string(),
            name,
            keys,
        });
    }
    saved
}

/// Applies one change. On an error `saved` is left as it was. On success the
/// result is the view to select, which is the new one after an add.
pub fn apply(saved: &mut Saved, change: &Change) -> Result<Option<(Group, String)>, String> {
    match change {
        Change::Add {
            group,
            name,
            keys,
            reserved,
        } => {
            let name = clean_name(name);
            let keys = clean_keys(keys);
            if name.is_empty() {
                return Err("Give the view a name.".into());
            }
            if keys.is_empty() {
                return Err("Pick at least one item for the view.".into());
            }
            let list = saved.list_mut(*group);
            if reserved.iter().any(|item| *item == name) || list.iter().any(|view| view.name == name) {
                return Err(format!("A view called \"{name}\" already exists."));
            }
            let next = list.iter().filter_map(|view| id_number(&view.id, *group)).max().unwrap_or(0) + 1;
            let id = format!("{}-{next}", group.tag());
            list.push(NamedView {
                id: id.clone(),
                name,
                keys,
            });
            if encode(saved).len() > MAX_BYTES {
                saved.list_mut(*group).pop();
                return Err("There is no room to save another view. Delete one first.".into());
            }
            Ok(Some((*group, id)))
        }
        Change::Remove { group, id } => {
            saved.list_mut(*group).retain(|view| view.id != *id);
            Ok(None)
        }
        Change::AddSignal(signal) => {
            let mut signal = signal.clone();
            signal.tidy();
            signal.check()?;
            if saved.signals.len() >= MAX_SIGNALS {
                return Err(format!("There is room for {MAX_SIGNALS} signals. Delete one first."));
            }
            if saved.signals.iter().any(|kept| kept.name == signal.name) {
                return Err(format!("A signal called \"{}\" already exists.", signal.name));
            }
            let next = saved.signals.iter().filter_map(|kept| signal_number(&kept.key)).max().unwrap_or(0) + 1;
            signal.key = format!("signal-{next}");
            saved.signals.push(signal);
            if encode(saved).len() > MAX_BYTES {
                saved.signals.pop();
                return Err("There is no room to save another signal. Delete one first.".into());
            }
            Ok(None)
        }
        Change::RemoveSignal { key } => {
            saved.signals.retain(|signal| signal.key != *key);
            Ok(None)
        }
    }
}

/// What came of a load or a change.
#[derive(Debug)]
pub struct Outcome {
    /// True for the load at start-up, whose failure needs no message.
    pub load: bool,
    /// The list as stored now. `None` when the store could not be reached.
    pub saved: Option<Saved>,
    pub selected: Option<(Group, String)>,
    /// The change to apply to the list in memory, when it could not be stored.
    pub unsaved: Option<Change>,
    pub error: Option<String>,
}

impl Outcome {
    fn unreachable(change: Option<Change>, reason: &str) -> Self {
        let load = change.is_none();
        Self {
            load,
            saved: None,
            selected: None,
            error: Some(format!(
                "Could not use the saved views ({reason}). Views last only until you close the {SESSION_WORD}."
            )),
            unsaved: change,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
const SESSION_WORD: &str = "app";
#[cfg(target_arch = "wasm32")]
const SESSION_WORD: &str = "page";

/// The result of changing a stored list: what to show, and what to write back.
pub struct Run {
    pub saved: Saved,
    pub selected: Option<(Group, String)>,
    pub error: Option<String>,
    pub write: Option<String>,
}

pub fn run_change(current: &str, change: &Change) -> Run {
    let mut saved = decode(current);
    let before = encode(&saved);
    match apply(&mut saved, change) {
        Ok(selected) => {
            let after = encode(&saved);
            Run {
                write: (after != before).then_some(after),
                saved,
                selected,
                error: None,
            }
        }
        Err(error) => Run {
            saved,
            selected: None,
            error: Some(error),
            write: None,
        },
    }
}

static OUTCOMES: Mutex<Vec<Outcome>> = Mutex::new(Vec::new());

fn post(outcome: Outcome) {
    OUTCOMES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push(outcome);
}

/// Answers that arrived since the last call, oldest first.
pub fn take_outcomes() -> Vec<Outcome> {
    std::mem::take(&mut *OUTCOMES.lock().unwrap_or_else(|poison| poison.into_inner()))
}

/// Starts reading the stored list. The answer comes through `take_outcomes`.
pub fn load() {
    backend::load();
}

/// Starts a change. The answer comes through `take_outcomes`.
pub fn change(change: Change) {
    backend::change(change);
}

#[cfg(not(target_arch = "wasm32"))]
mod backend {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::{decode, post, run_change, Change, Outcome};

    static BUSY: Mutex<()> = Mutex::new(());

    pub fn load() {
        let _turn = BUSY.lock().unwrap_or_else(|poison| poison.into_inner());
        post(match store_path() {
            Some(path) => load_at(&path),
            None => Outcome::unreachable(None, "no settings folder"),
        });
    }

    pub fn change(change: Change) {
        let _turn = BUSY.lock().unwrap_or_else(|poison| poison.into_inner());
        post(match store_path() {
            Some(path) => change_at(&path, change),
            None => Outcome::unreachable(Some(change), "no settings folder"),
        });
    }

    pub(super) fn load_at(path: &Path) -> Outcome {
        match read_at(path) {
            Ok(text) => Outcome {
                load: true,
                saved: Some(decode(&text)),
                selected: None,
                unsaved: None,
                error: None,
            },
            Err(reason) => Outcome::unreachable(None, &reason),
        }
    }

    pub(super) fn change_at(path: &Path, change: Change) -> Outcome {
        let current = match read_at(path) {
            Ok(text) => text,
            Err(reason) => return Outcome::unreachable(Some(change), &reason),
        };
        let run = run_change(&current, &change);
        if let Some(text) = &run.write {
            if let Err(reason) = write_at(path, text) {
                return Outcome::unreachable(Some(change), &reason);
            }
        }
        Outcome {
            load: false,
            saved: Some(run.saved),
            selected: run.selected,
            unsaved: None,
            error: run.error,
        }
    }

    fn read_at(path: &Path) -> Result<String, String> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(err) => Err(format!("could not read {}: {err}", path.display())),
        }
    }

    /// Writes beside the file and renames over it, so a crash part-way never
    /// leaves half a list.
    fn write_at(path: &Path, text: &str) -> Result<(), String> {
        let fail = |err: std::io::Error| format!("could not write {}: {err}", path.display());
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(fail)?;
        }
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, text).map_err(fail)?;
        std::fs::rename(&temp, path).map_err(fail)
    }

    fn store_path() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            let home = std::env::var_os("HOME")?;
            Some(PathBuf::from(home).join("Library/Application Support/Silence S01/views.txt"))
        }
        #[cfg(windows)]
        {
            let data = std::env::var_os("APPDATA")?;
            Some(PathBuf::from(data).join("Silence S01").join("views.txt"))
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
            Some(base.join("silence-s01").join("views.txt"))
        }
    }
}

/// In a browser the list lives on the device that served the page. Jobs run
/// one at a time, so two quick clicks cannot read the same old list.
#[cfg(target_arch = "wasm32")]
mod backend {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use eframe::wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    use super::{decode, post, run_change, Change, Outcome};

    const URL: &str = "/views";

    /// `None` is a load.
    static QUEUE: Mutex<VecDeque<Option<Change>>> = Mutex::new(VecDeque::new());
    static RUNNING: AtomicBool = AtomicBool::new(false);

    pub fn load() {
        enqueue(None);
    }

    pub fn change(change: Change) {
        enqueue(Some(change));
    }

    fn enqueue(job: Option<Change>) {
        QUEUE
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push_back(job);
        if !RUNNING.swap(true, Ordering::SeqCst) {
            wasm_bindgen_futures::spawn_local(async {
                loop {
                    let next = QUEUE.lock().unwrap_or_else(|poison| poison.into_inner()).pop_front();
                    match next {
                        Some(job) => run(job).await,
                        None => {
                            RUNNING.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                }
            });
        }
    }

    async fn run(job: Option<Change>) {
        let current = match get().await {
            Ok(text) => text,
            Err(reason) => {
                post(Outcome::unreachable(job, &reason));
                return;
            }
        };
        let Some(change) = job else {
            post(Outcome {
                load: true,
                saved: Some(decode(&current)),
                selected: None,
                unsaved: None,
                error: None,
            });
            return;
        };
        let run = run_change(&current, &change);
        if let Some(text) = &run.write {
            if let Err(reason) = put(text).await {
                post(Outcome::unreachable(Some(change), &reason));
                return;
            }
        }
        post(Outcome {
            load: false,
            saved: Some(run.saved),
            selected: run.selected,
            unsaved: None,
            error: run.error,
        });
    }

    async fn send(init: &web_sys::RequestInit) -> Result<web_sys::Response, String> {
        let window = web_sys::window().ok_or_else(|| "no browser window".to_string())?;
        let answer = JsFuture::from(window.fetch_with_str_and_init(URL, init))
            .await
            .map_err(|_| "the device did not answer".to_string())?;
        let response = answer
            .dyn_into::<web_sys::Response>()
            .map_err(|_| "the device sent something unexpected".to_string())?;
        if response.ok() {
            Ok(response)
        } else {
            Err(format!("the device answered {}", response.status()))
        }
    }

    async fn get() -> Result<String, String> {
        let init = web_sys::RequestInit::new();
        init.set_method("GET");
        init.set_cache(web_sys::RequestCache::NoStore);
        let response = send(&init).await?;
        let text = response.text().map_err(|_| "the answer could not be read".to_string())?;
        let text = JsFuture::from(text)
            .await
            .map_err(|_| "the answer could not be read".to_string())?;
        Ok(text.as_string().unwrap_or_default())
    }

    async fn put(text: &str) -> Result<(), String> {
        let init = web_sys::RequestInit::new();
        init.set_method("PUT");
        init.set_cache(web_sys::RequestCache::NoStore);
        init.set_body(&JsValue::from_str(text));
        send(&init).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(group: Group, name: &str, keys: &[&str]) -> Change {
        Change::Add {
            group,
            name: name.into(),
            keys: keys.iter().map(|key| key.to_string()).collect(),
            reserved: vec!["Pack".into(), "All".into()],
        }
    }

    fn names(saved: &Saved, group: Group) -> Vec<String> {
        saved.list(group).iter().map(|view| view.name.clone()).collect()
    }

    #[test]
    fn a_list_survives_being_written_and_read() {
        let mut saved = Saved::default();
        apply(&mut saved, &add(Group::Live, "Commute", &["speed", "range"])).unwrap();
        apply(&mut saved, &add(Group::Reading, "Cells only", &["cell0", "cell1", "cell13"])).unwrap();
        let again = decode(&encode(&saved));
        assert_eq!(again, saved);
        assert_eq!(again.live[0].id, "live-1");
        assert_eq!(again.reading[0].id, "reading-1");
    }

    #[test]
    fn a_new_view_is_the_one_selected() {
        let mut saved = Saved::default();
        let selected = apply(&mut saved, &add(Group::Live, "Commute", &["speed"])).unwrap();
        assert_eq!(selected, Some((Group::Live, "live-1".to_string())));
    }

    #[test]
    fn a_new_id_never_reuses_one_left_behind_by_a_delete() {
        let mut saved = Saved::default();
        apply(&mut saved, &add(Group::Live, "One", &["speed"])).unwrap();
        apply(&mut saved, &add(Group::Live, "Two", &["rpm"])).unwrap();
        apply(
            &mut saved,
            &Change::Remove {
                group: Group::Live,
                id: "live-1".into(),
            },
        )
        .unwrap();
        apply(&mut saved, &add(Group::Live, "Three", &["soc"])).unwrap();
        let ids: Vec<&str> = saved.live.iter().map(|view| view.id.as_str()).collect();
        assert_eq!(ids, ["live-2", "live-3"]);
    }

    #[test]
    fn deleting_removes_only_that_view_and_a_second_delete_is_harmless() {
        let mut saved = Saved::default();
        apply(&mut saved, &add(Group::Reading, "A", &["soc"])).unwrap();
        apply(&mut saved, &add(Group::Reading, "B", &["speed"])).unwrap();
        let remove = Change::Remove {
            group: Group::Reading,
            id: "reading-1".into(),
        };
        apply(&mut saved, &remove).unwrap();
        assert_eq!(names(&saved, Group::Reading), ["B"]);
        assert_eq!(apply(&mut saved, &remove), Ok(None));
        assert_eq!(names(&saved, Group::Reading), ["B"]);
    }

    #[test]
    fn the_same_name_may_be_used_once_per_screen_and_never_for_a_built_in() {
        let mut saved = Saved::default();
        apply(&mut saved, &add(Group::Live, "Commute", &["speed"])).unwrap();
        assert!(apply(&mut saved, &add(Group::Live, " Commute ", &["rpm"])).is_err());
        assert!(apply(&mut saved, &add(Group::Live, "Pack", &["rpm"])).is_err());
        assert!(apply(&mut saved, &add(Group::Reading, "Commute", &["soc"])).is_ok());
        assert_eq!(saved.live.len(), 1);
    }

    #[test]
    fn a_view_needs_a_name_and_something_in_it() {
        let mut saved = Saved::default();
        assert!(apply(&mut saved, &add(Group::Live, "   ", &["speed"])).is_err());
        assert!(apply(&mut saved, &add(Group::Live, "Empty", &[])).is_err());
        assert!(apply(&mut saved, &add(Group::Live, "Odd", &["not a key", ""])).is_err());
        assert_eq!(saved, Saved::default());
    }

    #[test]
    fn names_cannot_break_the_file() {
        let mut saved = Saved::default();
        apply(&mut saved, &add(Group::Live, "a\tb\nc", &["speed"])).unwrap();
        assert_eq!(saved.live[0].name, "a b c");
        let long = "x".repeat(100);
        apply(&mut saved, &add(Group::Live, &long, &["rpm"])).unwrap();
        assert_eq!(saved.live[1].name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(decode(&encode(&saved)), saved);
    }

    #[test]
    fn a_full_list_refuses_one_more_and_stays_as_it_was() {
        let mut saved = Saved::default();
        let keys: Vec<String> = (0..60).map(|index| format!("reading{index}")).collect();
        let mut count = 0;
        loop {
            let change = Change::Add {
                group: Group::Reading,
                name: format!("View {count}"),
                keys: keys.clone(),
                reserved: Vec::new(),
            };
            let before = saved.clone();
            if apply(&mut saved, &change).is_err() {
                assert_eq!(saved, before);
                break;
            }
            count += 1;
            assert!(count < 100, "the limit never applied");
        }
        assert!(count >= 5, "only {count} large views fit");
        assert!(encode(&saved).len() <= MAX_BYTES);
    }

    #[test]
    fn damaged_lines_are_skipped_and_the_rest_kept() {
        let text = "S01 views 1\n\
                    live\tlive-1\tGood\tspeed,range\n\
                    nonsense\n\
                    live\tbad id\tNo\tspeed\n\
                    live\tlive-2\tNo keys\t\n\
                    live\tlive-1\tDuplicate id\tspeed\n\
                    live\tlive-3\tGood\tsoc\n\
                    pie\tlive-4\tWrong group\tsoc\n\
                    reading\treading-7\tCells\tcell0,cell1\n";
        let saved = decode(text);
        assert_eq!(names(&saved, Group::Live), ["Good"]);
        assert_eq!(names(&saved, Group::Reading), ["Cells"]);
        assert_eq!(decode("something else\nlive\tlive-1\tX\tspeed\n"), Saved::default());
        assert_eq!(decode(""), Saved::default());
    }

    #[test]
    fn nothing_is_written_when_nothing_changed() {
        let first = run_change("", &add(Group::Live, "Commute", &["speed"]));
        let text = first.write.expect("the first add writes");
        let missing = run_change(
            &text,
            &Change::Remove {
                group: Group::Live,
                id: "live-9".into(),
            },
        );
        assert!(missing.write.is_none());
        let refused = run_change(&text, &add(Group::Live, "Commute", &["rpm"]));
        assert!(refused.write.is_none());
        assert!(refused.error.is_some());
        assert_eq!(names(&refused.saved, Group::Live), ["Commute"]);
    }

    fn signal(name: &str) -> SignalDef {
        SignalDef {
            key: String::new(),
            name: name.into(),
            id: 0x182,
            start: 2,
            count: 2,
            signed: true,
            big_endian: false,
            scale: 0.1,
            offset: -40.0,
            unit: "C".into(),
        }
    }

    #[test]
    fn a_signal_survives_being_written_and_read() {
        let mut saved = Saved::default();
        apply(&mut saved, &Change::AddSignal(signal("Horn"))).unwrap();
        let mut odd = signal("Odd, one");
        odd.big_endian = true;
        odd.signed = false;
        odd.scale = 0.0625;
        odd.unit = String::new();
        apply(&mut saved, &Change::AddSignal(odd)).unwrap();
        apply(&mut saved, &add(Group::Live, "Commute", &["speed"])).unwrap();
        let again = decode(&encode(&saved));
        assert_eq!(again, saved);
        assert_eq!(again.signals[0].key, "signal-1");
        assert_eq!(again.signals[1].key, "signal-2");
        assert_eq!(again.signals[1].scale, 0.0625);
        assert!(again.signals[1].big_endian && !again.signals[1].signed);
    }

    #[test]
    fn a_new_signal_key_never_reuses_one_left_behind_by_a_delete() {
        let mut saved = Saved::default();
        apply(&mut saved, &Change::AddSignal(signal("One"))).unwrap();
        apply(&mut saved, &Change::AddSignal(signal("Two"))).unwrap();
        apply(&mut saved, &Change::RemoveSignal { key: "signal-1".into() }).unwrap();
        apply(&mut saved, &Change::AddSignal(signal("Three"))).unwrap();
        let keys: Vec<&str> = saved.signals.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["signal-2", "signal-3"]);
        // A second delete of the same key is harmless.
        apply(&mut saved, &Change::RemoveSignal { key: "signal-1".into() }).unwrap();
        assert_eq!(saved.signals.len(), 2);
    }

    #[test]
    fn a_signal_must_be_readable_and_have_its_own_name() {
        let mut saved = Saved::default();
        apply(&mut saved, &Change::AddSignal(signal("Horn"))).unwrap();
        assert!(apply(&mut saved, &Change::AddSignal(signal(" Horn "))).is_err());
        let mut nameless = signal("   ");
        assert!(apply(&mut saved, &Change::AddSignal(nameless.clone())).is_err());
        nameless.name = "ok".into();
        nameless.start = 7;
        assert!(apply(&mut saved, &Change::AddSignal(nameless)).is_err());
        let mut zero = signal("Zero");
        zero.scale = 0.0;
        assert!(apply(&mut saved, &Change::AddSignal(zero)).is_err());
        assert_eq!(saved.signals.len(), 1);
    }

    #[test]
    fn only_so_many_signals_can_be_kept() {
        let mut saved = Saved::default();
        for index in 0..MAX_SIGNALS {
            apply(&mut saved, &Change::AddSignal(signal(&format!("S{index}")))).unwrap();
        }
        let before = saved.clone();
        assert!(apply(&mut saved, &Change::AddSignal(signal("One too many"))).is_err());
        assert_eq!(saved, before);
        assert!(encode(&saved).len() <= MAX_BYTES);
    }

    #[test]
    fn damaged_signal_lines_are_skipped_and_the_rest_kept() {
        let text = "S01 views 1\n\
                    signal\tsignal-1\tGood\t386\t2\t2\t1\t0\t0.1\t-40\tC\n\
                    signal\tsignal-2\tToo short\t386\n\
                    signal\tsignal-3\tBad id\t2048\t0\t1\t0\t0\t1\t0\t\n\
                    signal\tsignal-4\tPast the end\t386\t7\t4\t0\t0\t1\t0\t\n\
                    signal\tsignal-5\tNot a number\t386\t0\t1\t0\t0\tx\t0\t\n\
                    signal\tsignal-6\tZero scale\t386\t0\t1\t0\t0\t0\t0\t\n\
                    signal\tsignal-1\tSame key\t386\t0\t1\t0\t0\t1\t0\t\n\
                    signal\tsignal-7\tGood\t386\t0\t1\t0\t0\t1\t0\t\n\
                    signal\tnot-a-key\tBad key\t386\t0\t1\t0\t0\t1\t0\t\n\
                    signal\tsignal-8\tFine\t386\t0\t1\t0\t0\t1\t0\t%\n";
        let saved = decode(text);
        let names: Vec<&str> = saved.signals.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Good", "Fine"]);
        assert_eq!(saved.signals[0].id, 386);
        assert_eq!(saved.signals[0].offset, -40.0);
        assert_eq!(saved.signals[1].unit, "%");
    }

    #[test]
    fn a_list_with_signals_still_loads_the_views_and_ignores_nothing_it_knows() {
        let mut saved = Saved::default();
        apply(&mut saved, &add(Group::Reading, "Cells", &["cell0"])).unwrap();
        apply(&mut saved, &Change::AddSignal(signal("Horn"))).unwrap();
        let text = encode(&saved);
        // The views part of the file is exactly what a version without
        // signals wrote, so such a version reads it and skips the rest.
        let views_only = text.lines().filter(|line| !line.starts_with("signal")).collect::<Vec<_>>().join("\n");
        let old = decode(&views_only);
        assert_eq!(names(&old, Group::Reading), ["Cells"]);
        assert!(old.signals.is_empty());
        assert_eq!(names(&decode(&text), Group::Reading), ["Cells"]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod on_disk {
        use super::*;
        use crate::views::backend::{change_at, load_at};

        fn temp(name: &str) -> std::path::PathBuf {
            let folder = std::env::temp_dir().join(format!("s01-views-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            folder.join("views.txt")
        }

        #[test]
        fn views_come_back_after_a_restart() {
            let path = temp("restart");
            assert_eq!(load_at(&path).saved, Some(Saved::default()));
            let added = change_at(&path, add(Group::Live, "Commute", &["speed", "range"]));
            assert_eq!(added.selected, Some((Group::Live, "live-1".to_string())));
            change_at(&path, add(Group::Reading, "Cells", &["cell0"]));
            let restarted = load_at(&path).saved.expect("saved list");
            assert_eq!(names(&restarted, Group::Live), ["Commute"]);
            assert_eq!(restarted.live[0].keys, ["speed", "range"]);
            assert_eq!(names(&restarted, Group::Reading), ["Cells"]);
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }

        #[test]
        fn a_deleted_view_stays_deleted_after_a_restart() {
            let path = temp("delete");
            change_at(&path, add(Group::Live, "One", &["speed"]));
            change_at(&path, add(Group::Live, "Two", &["rpm"]));
            let outcome = change_at(
                &path,
                Change::Remove {
                    group: Group::Live,
                    id: "live-1".into(),
                },
            );
            assert_eq!(names(outcome.saved.as_ref().unwrap(), Group::Live), ["Two"]);
            let restarted = load_at(&path).saved.expect("saved list");
            assert_eq!(names(&restarted, Group::Live), ["Two"]);
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }

        #[test]
        fn a_store_that_cannot_be_written_keeps_the_change_for_the_session() {
            let folder = std::env::temp_dir().join(format!("s01-views-test-blocked-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::write(&folder, "a file where the folder should be").unwrap();
            let path = folder.join("views.txt");
            let change = add(Group::Live, "Commute", &["speed"]);
            let outcome = change_at(&path, change.clone());
            assert!(outcome.saved.is_none());
            assert_eq!(outcome.unsaved, Some(change));
            assert!(outcome.error.is_some());
            let _ = std::fs::remove_file(&folder);
        }
    }
}
