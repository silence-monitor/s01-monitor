//! The SD card screen: the card on the unit's Atomic TF Card Base, seen from the
//! browser page the unit serves. It shows what the card is, lets a person
//! format it, eject it, browse it, download and upload files, and start and
//! stop the unit's own recording of the bus.
//!
//! The menu entry exists only when the unit says it has a card (`/info`), so it
//! never shows on a computer or with the other hardware.
//!
//! `SdScreen` holds the state and no drawing, so it can be tested on its own;
//! the drawing is `MonitorApp::sd_card` and the confirmations `sd_dialog`.

use eframe::egui::{self, Button, RichText, TextEdit};
use web_time::{Duration, Instant};

use super::{action_button, card, MonitorApp, ALARM, GOOD, INK, MUTED, WARN};
use crate::sdcard::{self, Answer, CardState, Kind, Listing, Request, Status, Upload};

const STATUS_EVERY: Duration = Duration::from_secs(2);
const TIME_EVERY: Duration = Duration::from_secs(300);
/// A request with no answer for this long is given up on, so one lost answer
/// cannot stop the polling for good.
const REQUEST_LOST: Duration = Duration::from_secs(6);
const LOOKING_FOR: Duration = Duration::from_secs(5);

/// A destructive action waiting for a yes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    Format,
    Eject,
    Delete { path: String, dir: bool },
}

#[derive(Debug)]
pub struct SdScreen {
    /// The unit said it has a card, so the screen has a menu entry.
    pub available: bool,
    pub hardware: String,
    asked: bool,
    pub status: Status,
    /// The unit has not answered the last status request.
    pub unreachable: bool,
    status_at: Option<Instant>,
    status_waiting: bool,
    pub path: String,
    pub listing: Option<Listing>,
    list_waiting: bool,
    list_at: Option<Instant>,
    list_wanted: bool,
    looking_until: Option<Instant>,
    pub list_error: Option<String>,
    time_at: Option<Instant>,
    /// What the last action said, and whether it was a problem.
    pub note: Option<(String, bool)>,
    pub confirm: Option<Confirm>,
    pub new_folder: String,
    pub upload: Upload,
    upload_at: Option<Instant>,
    last_busy: Option<String>,
    last_state: CardState,
}

impl Default for SdScreen {
    fn default() -> Self {
        Self {
            available: false,
            hardware: String::new(),
            asked: false,
            status: Status::default(),
            unreachable: false,
            status_at: None,
            status_waiting: false,
            path: "/".into(),
            listing: None,
            list_waiting: false,
            list_at: None,
            list_wanted: true,
            looking_until: None,
            list_error: None,
            time_at: None,
            note: None,
            confirm: None,
            new_folder: String::new(),
            upload: Upload::Idle,
            upload_at: None,
            last_busy: None,
            last_state: CardState::Unknown,
        }
    }
}

impl SdScreen {
    /// What to ask the unit now. `visible` is whether the screen is open: the
    /// unit is polled only then. `local_seconds` is this device's local time.
    pub fn tick(&mut self, now: Instant, visible: bool, local_seconds: Option<i64>) -> Vec<Request> {
        let mut requests = Vec::new();
        if !self.asked {
            self.asked = true;
            requests.push(sdcard::info());
        }
        if !self.available {
            return requests;
        }
        if let Some(seconds) = local_seconds {
            if self.time_at.map_or(true, |at| now.duration_since(at) >= TIME_EVERY) {
                self.time_at = Some(now);
                requests.push(sdcard::time(seconds));
            }
        }
        if self.status_waiting && self.status_at.map_or(false, |at| now.duration_since(at) >= REQUEST_LOST) {
            self.status_waiting = false;
        }
        if self.list_waiting && self.list_at.map_or(false, |at| now.duration_since(at) >= REQUEST_LOST) {
            self.list_waiting = false;
            self.list_wanted = true;
        }
        if visible {
            if !self.status_waiting && self.status_at.map_or(true, |at| now.duration_since(at) >= STATUS_EVERY) {
                self.status_at = Some(now);
                self.status_waiting = true;
                requests.push(sdcard::status());
            }
            if self.list_wanted && !self.list_waiting && self.status.state == CardState::Ready {
                self.list_wanted = false;
                self.list_waiting = true;
                self.list_at = Some(now);
                requests.push(sdcard::list(&self.path));
            }
        }
        requests
    }

    /// Takes in what the unit answered.
    pub fn apply(&mut self, answer: Answer) {
        match answer.kind {
            Kind::Info => {
                let info = sdcard::parse_info(&answer.text);
                self.available = answer.ok() && info.sd;
                self.hardware = info.hardware;
            }
            Kind::Time => {}
            Kind::Status => {
                self.status_waiting = false;
                if !answer.ok() {
                    self.unreachable = true;
                    return;
                }
                self.unreachable = false;
                let status = sdcard::parse_status(&answer.text);
                // A card that has just become ready, or a job that has just
                // finished (here or in another browser), changes what is on it.
                if (status.state == CardState::Ready && self.last_state != CardState::Ready)
                    || (self.last_busy.is_some() && status.busy.is_none())
                {
                    self.list_wanted = true;
                }
                if status.state != CardState::Ready {
                    self.listing = None;
                    self.list_error = None;
                }
                // The "formatting has started" note has done its job once the unit is done.
                if status.busy.is_none()
                    && status.state != CardState::Formatting
                    && self.note.as_ref().is_some_and(|(text, _)| text.starts_with("Formatting has started"))
                {
                    self.note = None;
                }
                self.last_state = status.state;
                self.last_busy = status.busy.clone();
                self.status = status;
            }
            Kind::List => {
                self.list_waiting = false;
                if answer.tag != self.path {
                    self.list_wanted = true; // the person moved on meanwhile
                    return;
                }
                if answer.ok() {
                    self.listing = Some(sdcard::parse_listing(&answer.text));
                    self.list_error = None;
                } else if answer.status != 503 {
                    self.list_error = Some(answer.problem());
                }
            }
            Kind::Mkdir | Kind::Delete | Kind::Format | Kind::Eject | Kind::Mount | Kind::Record | Kind::Auto => {
                if answer.ok() && answer.kind == Kind::Mount {
                    // The result shows in the card's state; say so only for a moment.
                    self.looking_until = Some(Instant::now() + LOOKING_FOR);
                } else if answer.ok() {
                    // Starting and stopping a recording show in the card's status; no note needed.
                    self.note = Some(done_text(&answer)).filter(|text| !text.is_empty()).map(|text| (text, false));
                    self.list_wanted = true;
                } else {
                    self.note = Some((answer.problem(), true));
                }
                self.status_at = None; // look at the status again at once
            }
        }
    }

    /// Opens a folder.
    pub fn open(&mut self, path: &str) {
        self.path = path.to_string();
        self.listing = None;
        self.list_error = None;
        self.list_wanted = true;
        self.note = None;
    }

    pub fn refresh(&mut self) {
        self.list_wanted = true;
        self.status_at = None;
    }

    /// Takes in how an upload the page is making stands.
    pub fn apply_upload(&mut self, now: Instant, state: Upload) {
        self.upload_at = Some(now);
        match &state {
            Upload::Finished { name } => {
                self.note = Some((format!("Uploaded {name}."), false));
                self.list_wanted = true;
                self.status_at = None;
            }
            Upload::Failed { name, why } => {
                let why = why.trim();
                self.note = Some((format!("Could not upload {name}: {}", if why.is_empty() { "the unit refused it" } else { why }), true));
                self.list_wanted = true;
                self.status_at = None;
            }
            _ => {}
        }
        self.upload = state;
    }

    pub fn wants_upload_poll(&self, now: Instant, visible: bool) -> bool {
        let sending = matches!(self.upload, Upload::Sending { .. });
        (sending || visible) && self.upload_at.map_or(true, |at| now.duration_since(at) >= Duration::from_millis(300))
    }

    /// "Look again" was pressed a moment ago and the unit has not answered with
    /// a new state yet.
    pub fn looking(&self, now: Instant) -> bool {
        self.looking_until.map_or(false, |until| now < until)
    }

    /// True while the unit is doing something that should not be interrupted.
    pub fn busy(&self) -> bool {
        self.status.busy.is_some() || matches!(self.upload, Upload::Sending { .. })
    }
}

fn done_text(answer: &Answer) -> String {
    match answer.kind {
        Kind::Mkdir => format!("Created the folder {}.", sdcard::leaf(&answer.tag)),
        Kind::Delete => format!("Deleted {}.", sdcard::leaf(&answer.tag)),
        Kind::Format => "Formatting has started. It can take up to a minute; the card's state is shown below.".to_string(),
        Kind::Eject => "Ejected. It is safe to take the card out.".to_string(),
        Kind::Record | Kind::Auto => String::new(),
        _ => "Done.".to_string(),
    }
}

// ---- The screen ------------------------------------------------------------------

impl MonitorApp {
    /// Called every frame: hands requests to the unit and takes in its answers.
    pub(super) fn sd_tick(&mut self) {
        for answer in sdcard::take_answers() {
            self.sd.apply(answer);
        }
        let now = Instant::now();
        let visible = self.screen == super::Screen::SdCard;
        if self.sd.wants_upload_poll(now, visible) {
            let state = sdcard::upload_state();
            if matches!(state, Upload::Finished { .. } | Upload::Failed { .. }) {
                sdcard::clear_upload();
            }
            self.sd.apply_upload(now, state);
        }
        for request in self.sd.tick(now, visible, sdcard::local_seconds()) {
            sdcard::send(request);
        }
    }

    pub(super) fn sd_card(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("SD card").size(18.0).color(INK));
        ui.add_space(4.0);
        ui.label(
            RichText::new("The card in the Atomic TF Card Base on the unit: its state, the unit's own recording of the bus, and the files on it.")
                .color(MUTED),
        );
        ui.add_space(8.0);
        if self.sd.unreachable {
            ui.label(RichText::new("The unit is not answering. This page may have lost its connection.").color(WARN));
            ui.add_space(6.0);
        }
        self.sd_note(ui);
        self.sd_status_card(ui);
        ui.add_space(10.0);
        self.sd_record_card(ui);
        if self.sd.status.state == CardState::Ready {
            ui.add_space(10.0);
            self.sd_files_card(ui);
        }
    }

    fn sd_note(&mut self, ui: &mut egui::Ui) {
        if let Some((text, bad)) = self.sd.note.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(text).color(if bad { ALARM } else { GOOD }));
                if ui.small_button("Dismiss").clicked() {
                    self.sd.note = None;
                }
            });
            ui.add_space(6.0);
        }
    }

    fn sd_status_card(&mut self, ui: &mut egui::Ui) {
        let status = self.sd.status.clone();
        let busy = self.sd.busy();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Card").size(15.0).color(INK));
            ui.add_space(4.0);
            let (headline, colour) = match status.state {
                CardState::Unknown => ("Asking the unit...", MUTED),
                CardState::NoCard => ("No card found", WARN),
                CardState::NoFilesystem => ("The card has no file system the unit can read", WARN),
                CardState::TooBig => ("This card is too big", WARN),
                CardState::Ejected => ("Ejected", MUTED),
                CardState::Ready => ("Ready", GOOD),
                CardState::Formatting => ("Formatting...", WARN),
                CardState::Unmounting => ("Letting go of the card...", WARN),
            };
            ui.label(RichText::new(headline).size(16.0).color(colour));
            if !status.message.is_empty() {
                ui.label(RichText::new(&status.message).color(MUTED));
            }
            if self.sd.looking(Instant::now()) && status.state != CardState::Ready {
                ui.label(RichText::new("Looking for the card again...").color(MUTED));
            }
            if status.state == CardState::NoCard {
                ui.label(
                    RichText::new("Put a card of 16 GB or less in the Atomic TF Card Base. The unit looks again every few seconds.")
                        .color(MUTED),
                );
            }
            if status.state == CardState::Formatting {
                ui.label(RichText::new("This can take a minute. The card cannot be used until it is done.").color(MUTED));
            }
            if status.state == CardState::Ready {
                ui.add_space(6.0);
                egui::Grid::new("sd-facts").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    let fact = |ui: &mut egui::Ui, name: &str, value: String| {
                        ui.label(RichText::new(name).color(MUTED));
                        ui.label(RichText::new(value).color(INK));
                        ui.end_row();
                    };
                    let card_name = if status.card_name.is_empty() { "-".to_string() } else { status.card_name.clone() };
                    fact(ui, "Card", format!("{} {}", status.card_type, card_name).trim().to_string());
                    fact(ui, "Size", sdcard::size_text(status.card_bytes));
                    if status.space_known {
                        fact(ui, "Used", sdcard::size_text(status.used_bytes()));
                        fact(ui, "Free", sdcard::size_text(status.free_bytes));
                    } else {
                        fact(ui, "Free space", "working it out...".to_string());
                    }
                });
                if status.space_known && status.total_bytes > 0 {
                    ui.add_space(6.0);
                    let used = status.used_bytes() as f32 / status.total_bytes as f32;
                    ui.add(egui::ProgressBar::new(used.clamp(0.0, 1.0)).desired_width(ui.available_width().min(420.0)));
                }
            }
            if let Some(job) = &status.busy {
                ui.add_space(6.0);
                let mut text = format!("The unit is busy: {job}");
                if status.busy_total > 0 && matches!(job.as_str(), "download" | "upload") {
                    text.push_str(&format!(" ({} of {})", sdcard::size_text(status.busy_done), sdcard::size_text(status.busy_total)));
                }
                ui.label(RichText::new(text).color(WARN));
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let ready = status.state == CardState::Ready;
                let can_format = (ready || status.state == CardState::NoFilesystem) && !busy && !status.recording && !status.record_wanted;
                if ready {
                    if action_button(ui, "Eject...", !busy).clicked() {
                        self.sd.confirm = Some(Confirm::Eject);
                    }
                }
                if matches!(status.state, CardState::Ejected | CardState::NoCard | CardState::TooBig | CardState::NoFilesystem)
                    && action_button(ui, if status.state == CardState::Ejected { "Use the card again" } else { "Look again" }, !busy)
                        .clicked()
                {
                    sdcard::send(sdcard::mount());
                }
                if ready || status.state == CardState::NoFilesystem {
                    if action_button(ui, "Format...", can_format).clicked() {
                        self.sd.confirm = Some(Confirm::Format);
                    }
                }
                if ready && action_button(ui, "Refresh", !busy).clicked() {
                    self.sd.refresh();
                }
            });
            if (status.recording || status.record_wanted) && (status.state == CardState::Ready) {
                ui.label(RichText::new("Stop the recording first to format the card.").color(MUTED));
            }
            if status.heap_free > 0 {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("The unit has {} KB of memory free.", status.heap_free / 1000)).size(11.0).color(MUTED));
            }
        });
    }

    fn sd_record_card(&mut self, ui: &mut egui::Ui) {
        let status = self.sd.status.clone();
        let ready = status.state == CardState::Ready;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Recording to the card").size(15.0).color(INK));
            ui.add_space(4.0);
            let line = if status.recording {
                format!(
                    "Recording to {}: {} frames, {}{}",
                    status.record_file,
                    status.record_frames,
                    sdcard::size_text(status.record_bytes),
                    if status.record_lost > 0 { format!(", {} lost", status.record_lost) } else { String::new() }
                )
            } else if status.record_wanted {
                "Waiting for the first frame from the bus...".to_string()
            } else if ready {
                "Not recording.".to_string()
            } else {
                "Not recording: there is no card ready.".to_string()
            };
            ui.label(RichText::new(line).color(if status.recording { GOOD } else { INK }));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let on = status.recording || status.record_wanted;
                if on {
                    if ui.button("Stop recording").clicked() {
                        sdcard::send(sdcard::record(false));
                    }
                } else if action_button(ui, "Start recording", ready && status.busy.as_deref() != Some("format")).clicked() {
                    sdcard::send(sdcard::record(true));
                }
                let mut automatic = status.auto;
                if ui
                    .checkbox(&mut automatic, "Start recording by itself whenever the unit has a card")
                    .on_hover_text("The unit records from power-up with no phone connected.")
                    .changed()
                {
                    sdcard::send(sdcard::auto(automatic));
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Recordings are text files in the recordings folder, in the format this app records. Download one and replay it with Replay a capture.",
                )
                .size(12.0)
                .color(MUTED),
            );
            if !status.clock {
                ui.label(
                    RichText::new(
                        "The unit's clock is not set yet. Until a page sets it, recordings are numbered (rec-0001.log) and their times count from power-up.",
                    )
                    .size(12.0)
                    .color(WARN),
                );
            } else {
                ui.label(
                    RichText::new("The unit's clock is set from the device that opened this page, so recordings carry that device's time of day.")
                        .size(12.0)
                        .color(MUTED),
                );
            }
        });
    }

    fn sd_files_card(&mut self, ui: &mut egui::Ui) {
        let phone = self.phone();
        let busy = self.sd.busy();
        let path = self.sd.path.clone();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Files").size(15.0).color(INK));
            ui.add_space(4.0);
            // Where we are, as buttons that lead back up.
            ui.horizontal_wrapped(|ui| {
                if ui.add(Button::new(RichText::new("Card").color(GOOD)).frame(false)).clicked() {
                    self.sd.open("/");
                }
                for (name, target) in sdcard::crumbs(&path) {
                    ui.label(RichText::new("/").color(MUTED));
                    if ui.add(Button::new(RichText::new(name).color(GOOD)).frame(false)).clicked() {
                        self.sd.open(&target);
                    }
                }
            });
            ui.add_space(6.0);
            if let Some(why) = self.sd.list_error.clone() {
                ui.label(RichText::new(why).color(ALARM));
            }
            let listing = self.sd.listing.clone();
            match listing {
                None if self.sd.list_error.is_none() => {
                    ui.label(RichText::new("Reading the folder...").color(MUTED));
                }
                None => {}
                Some(listing) => self.sd_entries(ui, &path, &listing, phone, busy),
            }
            ui.add_space(8.0);
            self.sd_folder_tools(ui, &path, busy);
        });
    }

    fn sd_entries(&mut self, ui: &mut egui::Ui, path: &str, listing: &Listing, phone: bool, busy: bool) {
        if listing.entries.is_empty() {
            ui.label(RichText::new("This folder is empty.").color(MUTED));
            return;
        }
        let mut open: Option<String> = None;
        let mut download: Option<String> = None;
        let mut delete: Option<(String, bool)> = None;
        let mut buttons = |ui: &mut egui::Ui, entry: &sdcard::Entry| {
            let target = sdcard::join(path, &entry.name);
            ui.horizontal(|ui| {
                if !entry.dir && ui.button("Download").clicked() {
                    download = Some(target.clone());
                }
                if action_button(ui, "Delete...", !busy).clicked() {
                    delete = Some((target.clone(), entry.dir));
                }
            });
        };
        if phone {
            for entry in &listing.entries {
                ui.add_space(4.0);
                if entry.dir {
                    if ui.add(Button::new(RichText::new(format!("{}/", entry.name)).color(GOOD)).frame(false)).clicked() {
                        open = Some(sdcard::join(path, &entry.name));
                    }
                } else {
                    ui.label(RichText::new(&entry.name).color(INK));
                }
                let detail = if entry.dir { entry.date.clone() } else { format!("{}  {}", sdcard::size_text(entry.size), entry.date) };
                ui.label(RichText::new(detail.trim().to_string()).size(12.0).color(MUTED));
                buttons(ui, entry);
                ui.separator();
            }
        } else {
            egui::Grid::new("sd-files").num_columns(4).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
                for entry in &listing.entries {
                    if entry.dir {
                        if ui.add(Button::new(RichText::new(format!("{}/", entry.name)).color(GOOD)).frame(false)).clicked() {
                            open = Some(sdcard::join(path, &entry.name));
                        }
                        ui.label("");
                    } else {
                        ui.label(RichText::new(&entry.name).color(INK));
                        ui.label(RichText::new(sdcard::size_text(entry.size)).color(MUTED));
                    }
                    ui.label(RichText::new(&entry.date).color(MUTED));
                    buttons(ui, entry);
                    ui.end_row();
                }
            });
        }
        if listing.more {
            ui.label(RichText::new("The folder holds more than this; only the first 300 items are shown.").size(12.0).color(WARN));
        }
        if let Some(target) = open {
            self.sd.open(&target);
        }
        if let Some(target) = download {
            sdcard::download(&target);
            self.sd.note = Some((format!("Download started: your browser will save {}.", sdcard::leaf(&target)), false));
        }
        if let Some((target, dir)) = delete {
            self.sd.confirm = Some(Confirm::Delete { path: target, dir });
        }
    }

    fn sd_folder_tools(&mut self, ui: &mut egui::Ui, path: &str, busy: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.add(TextEdit::singleline(&mut self.sd.new_folder).hint_text("New folder name").desired_width(170.0));
            let problem = sdcard::check_name(&self.sd.new_folder).err();
            let typed = !self.sd.new_folder.is_empty();
            if action_button(ui, "Create folder", typed && problem.is_none() && !busy).clicked() {
                sdcard::send(sdcard::mkdir(&sdcard::join(path, &self.sd.new_folder)));
                self.sd.new_folder.clear();
            }
            if typed {
                if let Some(why) = problem {
                    ui.label(RichText::new(why).size(12.0).color(WARN));
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let sending = matches!(self.sd.upload, Upload::Sending { .. });
            if action_button(ui, "Upload a file here...", !busy).clicked() {
                self.sd.note = None;
                sdcard::pick_upload(path);
            }
            if let Upload::Sending { name, done, total } = &self.sd.upload {
                let fraction = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                ui.add(
                    egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                        .desired_width(200.0)
                        .text(format!("{name}: {} of {}", sdcard::size_text(*done), sdcard::size_text(*total))),
                );
            } else if !sending {
                ui.label(RichText::new("Names: plain letters, digits, spaces and . _ - ( ) only.").size(12.0).color(MUTED));
            }
        });
    }

    /// Asks before something is erased, and sends it on a yes.
    pub(super) fn sd_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.sd.confirm.clone() else {
            return;
        };
        let status = &self.sd.status;
        let (title, lines, label): (String, Vec<String>, &str) = match &confirm {
            Confirm::Format => (
                "Format the card?".to_string(),
                vec![
                    if status.state == CardState::Ready {
                        format!("Everything on the card will be erased: {} in use of {}.", sdcard::size_text(status.used_bytes()), sdcard::size_text(status.total_bytes))
                    } else {
                        "The card has no file system the unit can read. Anything on it that another system understands will be erased.".to_string()
                    },
                    "This cannot be undone. It can take a minute, and the card cannot be used until it is done.".to_string(),
                ],
                "Format the card",
            ),
            Confirm::Eject => (
                "Eject the card?".to_string(),
                vec!["Any recording stops and the unit lets go of the card. It is then safe to take it out.".to_string()],
                "Eject",
            ),
            Confirm::Delete { path, dir } => (
                format!("Delete {}?", sdcard::leaf(path)),
                vec![
                    if *dir { "The folder is deleted with everything in it.".to_string() } else { "The file is deleted from the card.".to_string() },
                    "This cannot be undone.".to_string(),
                ],
                "Delete",
            ),
        };
        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::unique("sd-confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new(title).size(18.0).color(INK));
            ui.add_space(6.0);
            for line in &lines {
                ui.label(RichText::new(line).color(MUTED));
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let danger = !matches!(confirm, Confirm::Eject);
                let colour = if danger { ALARM } else { INK };
                if ui.add(Button::new(RichText::new(label).color(colour))).clicked() {
                    confirmed = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        });
        if confirmed {
            self.sd.confirm = None;
            self.sd.note = None;
            sdcard::send(match confirm {
                Confirm::Format => sdcard::format(),
                Confirm::Eject => sdcard::eject(),
                Confirm::Delete { path, .. } => sdcard::delete(&path),
            });
        } else if cancelled || modal.should_close() {
            self.sd.confirm = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(kind: Kind, tag: &str, status: u16, text: &str) -> Answer {
        Answer { kind, tag: tag.into(), status, text: text.into() }
    }

    fn ready_screen() -> SdScreen {
        let mut s = SdScreen::default();
        s.apply(answer(Kind::Info, "", 200, "version 0.13.0\nhardware x\nsd 1\n"));
        s.apply(answer(Kind::Status, "", 200, "state ready\nspace_known 1\ntotal_bytes 1000\nfree_bytes 400\n"));
        s
    }

    #[test]
    fn the_unit_is_asked_what_it_is_once_and_nothing_else_until_it_has_a_card() {
        let mut s = SdScreen::default();
        let now = Instant::now();
        let first = s.tick(now, true, Some(1_795_124_701));
        assert_eq!(first.iter().map(|r| r.kind).collect::<Vec<_>>(), [Kind::Info]);
        assert!(s.tick(now, true, Some(1_795_124_701)).is_empty());
        s.apply(answer(Kind::Info, "", 200, "sd 0\n"));
        assert!(!s.available);
        assert!(s.tick(now + Duration::from_secs(10), true, None).is_empty());
    }

    #[test]
    fn a_page_that_is_not_the_unit_never_gets_the_screen() {
        let mut s = SdScreen::default();
        s.apply(answer(Kind::Info, "", 404, "version 0.13.0\nsd 1\n"));
        assert!(!s.available);
        s.apply(answer(Kind::Info, "", 0, "no answer"));
        assert!(!s.available);
    }

    #[test]
    fn the_clock_is_sent_once_and_again_every_five_minutes() {
        let mut s = SdScreen::default();
        let now = Instant::now();
        s.tick(now, false, Some(5));
        s.apply(answer(Kind::Info, "", 200, "sd 1\n"));
        let times = |requests: &[Request]| requests.iter().filter(|r| r.kind == Kind::Time).count();
        assert_eq!(times(&s.tick(now, false, Some(1_795_124_701))), 1);
        assert_eq!(times(&s.tick(now + Duration::from_secs(60), false, Some(1_795_124_761))), 0);
        assert_eq!(times(&s.tick(now + Duration::from_secs(301), false, Some(1_795_125_002))), 1);
        assert_eq!(times(&s.tick(now + Duration::from_secs(602), false, None)), 0);
    }

    #[test]
    fn status_is_polled_only_while_the_screen_is_open_and_never_two_at_once() {
        let mut s = SdScreen::default();
        let t0 = Instant::now();
        s.tick(t0, true, None);
        s.apply(answer(Kind::Info, "", 200, "sd 1\n"));
        let kinds = |r: Vec<Request>| r.into_iter().map(|r| r.kind).collect::<Vec<_>>();
        assert!(kinds(s.tick(t0, false, None)).is_empty());
        assert_eq!(kinds(s.tick(t0, true, None)), [Kind::Status]);
        assert!(kinds(s.tick(t0 + Duration::from_secs(5), true, None)).is_empty(), "the last one is unanswered");
        s.apply(answer(Kind::Status, "", 200, "state nocard\n"));
        assert!(kinds(s.tick(t0 + Duration::from_secs(1), true, None)).is_empty());
        assert_eq!(kinds(s.tick(t0 + Duration::from_secs(3), true, None)), [Kind::Status]);
    }

    #[test]
    fn a_ready_card_gets_its_folder_listed_and_a_finished_job_lists_it_again() {
        let mut s = SdScreen::default();
        let t0 = Instant::now();
        s.tick(t0, true, None);
        s.apply(answer(Kind::Info, "", 200, "sd 1\n"));
        s.tick(t0, true, None);
        s.apply(answer(Kind::Status, "", 200, "state ready\n"));
        let list = s.tick(t0 + Duration::from_secs(1), true, None);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, Kind::List);
        assert_eq!(list[0].tag, "/");
        s.apply(answer(Kind::List, "/", 200, "path /\nf\ta.txt\t5\t\n"));
        assert_eq!(s.listing.as_ref().map(|l| l.entries.len()), Some(1));
        // Nothing more until something changes.
        s.apply(answer(Kind::Status, "", 200, "state ready\n"));
        assert!(s.tick(t0 + Duration::from_secs(3), true, None).iter().all(|r| r.kind != Kind::List));
        // Another browser uploads: the unit is busy, then it is not.
        s.apply(answer(Kind::Status, "", 200, "state ready\nbusy upload\n"));
        s.apply(answer(Kind::Status, "", 200, "state ready\nbusy none\n"));
        assert!(s.tick(t0 + Duration::from_secs(9), true, None).iter().any(|r| r.kind == Kind::List));
    }

    #[test]
    fn an_answer_about_a_folder_the_person_has_left_is_not_shown() {
        let mut s = ready_screen();
        s.open("/b");
        s.apply(answer(Kind::List, "/", 200, "path /\nf\told\t1\t\n"));
        assert!(s.listing.is_none());
        assert!(s.list_wanted, "the folder they are in still has to be read");
        s.apply(answer(Kind::List, "/b", 200, "path /b\nf\tnew\t1\t\n"));
        assert_eq!(s.listing.as_ref().map(|l| l.entries[0].name.as_str()), Some("new"));
    }

    #[test]
    fn leaving_ready_forgets_the_listing() {
        let mut s = ready_screen();
        s.apply(answer(Kind::List, "/", 200, "path /\nf\ta\t1\t\n"));
        assert!(s.listing.is_some());
        s.apply(answer(Kind::Status, "", 200, "state ejected\n"));
        assert!(s.listing.is_none());
    }

    #[test]
    fn a_refused_action_says_why_and_a_good_one_says_what_happened() {
        let mut s = ready_screen();
        s.apply(answer(Kind::Delete, "/a/old.log", 409, "a recording is being written there; stop it first"));
        let (text, bad) = s.note.clone().unwrap();
        assert!(bad);
        assert_eq!(text, "A recording is being written there; stop it first.");
        s.apply(answer(Kind::Delete, "/a/old.log", 204, ""));
        assert_eq!(s.note, Some(("Deleted old.log.".to_string(), false)));
        s.apply(answer(Kind::Mkdir, "/a/new", 204, ""));
        assert_eq!(s.note, Some(("Created the folder new.".to_string(), false)));
        s.apply(answer(Kind::Format, "", 500, "formatting failed"));
        assert!(s.note.as_ref().unwrap().1);
        assert!(s.list_wanted);
    }

    #[test]
    fn an_unanswered_status_is_flagged_and_cleared_by_the_next_good_one() {
        let mut s = ready_screen();
        s.apply(answer(Kind::Status, "", 0, ""));
        assert!(s.unreachable);
        s.apply(answer(Kind::Status, "", 200, "state ready\n"));
        assert!(!s.unreachable);
    }

    #[test]
    fn a_lost_answer_does_not_stop_the_polling_for_good() {
        let mut s = SdScreen::default();
        let t0 = Instant::now();
        s.tick(t0, true, None);
        s.apply(answer(Kind::Info, "", 200, "sd 1\n"));
        let kinds = |r: Vec<Request>| r.into_iter().map(|r| r.kind).collect::<Vec<_>>();
        assert_eq!(kinds(s.tick(t0, true, None)), [Kind::Status]);
        assert!(kinds(s.tick(t0 + Duration::from_secs(4), true, None)).is_empty(), "still waiting");
        assert_eq!(kinds(s.tick(t0 + Duration::from_secs(7), true, None)), [Kind::Status], "given up on, asked again");
        // The same for a folder that never came back.
        s.apply(answer(Kind::Status, "", 200, "state ready\n"));
        let list = s.tick(t0 + Duration::from_secs(8), true, None);
        assert!(list.iter().any(|r| r.kind == Kind::List));
        s.apply(answer(Kind::Status, "", 200, "state ready\n"));
        assert!(s.tick(t0 + Duration::from_secs(10), true, None).iter().all(|r| r.kind != Kind::List));
        assert!(s.tick(t0 + Duration::from_secs(15), true, None).iter().any(|r| r.kind == Kind::List));
    }

    #[test]
    fn look_again_says_so_for_a_moment_and_leaves_no_note_behind() {
        let mut s = SdScreen::default();
        s.apply(answer(Kind::Mount, "", 204, ""));
        assert!(s.note.is_none());
        assert!(s.looking(Instant::now()));
        assert!(!s.looking(Instant::now() + Duration::from_secs(6)));
    }

    #[test]
    fn a_format_says_it_has_started_and_the_result_comes_from_the_status() {
        let mut s = ready_screen();
        s.apply(answer(Kind::Format, "", 204, ""));
        assert!(s.note.as_ref().is_some_and(|(text, bad)| text.starts_with("Formatting has started") && !bad));
        s.apply(answer(Kind::Status, "", 200, "state formatting\nbusy format\n"));
        assert!(s.busy());
        s.apply(answer(Kind::Status, "", 200, "state ready\nmessage Formatted.\nbusy none\n"));
        assert_eq!(s.status.message, "Formatted.");
        assert!(s.note.is_none(), "the note about it having started goes once it is done");
        assert!(s.list_wanted, "the card is new, so its folder is read again");
    }

    #[test]
    fn an_upload_that_ends_is_reported_and_refreshes_the_folder() {
        let mut s = ready_screen();
        let now = Instant::now();
        s.apply_upload(now, Upload::Sending { name: "a.bin".into(), done: 1, total: 2 });
        assert!(s.busy());
        s.list_wanted = false;
        s.apply_upload(now, Upload::Finished { name: "a.bin".into() });
        assert!(!s.busy());
        assert_eq!(s.note, Some(("Uploaded a.bin.".to_string(), false)));
        assert!(s.list_wanted);
        s.apply_upload(now, Upload::Failed { name: "b.bin".into(), why: "the card is full".into() });
        assert_eq!(s.note, Some(("Could not upload b.bin: the card is full".to_string(), true)));
    }

    #[test]
    fn the_upload_is_watched_while_it_runs_even_when_the_screen_is_closed() {
        let mut s = ready_screen();
        let now = Instant::now();
        assert!(!s.wants_upload_poll(now, false));
        assert!(s.wants_upload_poll(now, true));
        s.apply_upload(now, Upload::Sending { name: "a".into(), done: 0, total: 9 });
        assert!(!s.wants_upload_poll(now, true), "too soon");
        assert!(s.wants_upload_poll(now + Duration::from_millis(400), false));
    }
}
