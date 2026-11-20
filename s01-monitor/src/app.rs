use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use web_time::{Duration, Instant};

use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Frame, Layout, Margin, Panel, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Vec2,
};
mod explore;
mod sdscreen;

use crate::capture::{self, Recorder};
use crate::clock;
use crate::chart_image;
use crate::explain::{self, Tone};
use crate::export::{self, Table, ValueLogger};
use crate::frames::{self, Mark};
use crate::paramdiff::{self, Baseline, Change, ParamRow};
use crate::params::{self, Section};
use crate::platform;
use crate::readings::{self, mv_text, signed_mv, CellSpread, Row};
use crate::sdcard;
use crate::slcan;
use crate::snapshot::{self, lock, Sample, Snapshot};
use crate::source::{self, Kind};
use crate::stored::{self, Ask};
use crate::signals::SignalDef;
use crate::views::{self, Group, NamedView, Saved};

/// Width kept clear on the right of the scrolling pages for the scroll bar.
const SCROLL_GUTTER: f32 = 16.0;
/// The version of the app, from the project's top-level Cargo.toml.
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The menu on the left, open and collapsed to just its arrow.
const NAV_WIDTH: f32 = 168.0;
const NAV_COLLAPSED_WIDTH: f32 = 44.0;
/// Narrower than this is a phone held upright; narrower than `WIDE_FROM` the
/// left menu starts out collapsed.
const PHONE_BELOW: f32 = 600.0;
const WIDE_FROM: f32 = 900.0;
const TAB_BAR_HEIGHT: f32 = 56.0;
/// Heights of the stream chart as an overlay and as split panels.
const CHART_HEIGHTS: (f32, f32) = (280.0, 168.0);
const CHART_HEIGHTS_PHONE: (f32, f32) = (200.0, 140.0);
const BG: Color32 = Color32::from_rgb(0x10, 0x14, 0x10);
const PANEL: Color32 = Color32::from_rgb(0x17, 0x1c, 0x18);
const PANEL2: Color32 = Color32::from_rgb(0x1e, 0x25, 0x20);
const LINE_COLOR: Color32 = Color32::from_rgb(0x2c, 0x38, 0x30);
const INK: Color32 = Color32::from_rgb(0xe7, 0xec, 0xe4);
const MUTED: Color32 = Color32::from_rgb(0x8d, 0x9a, 0x90);
const GOOD: Color32 = Color32::from_rgb(0x9d, 0xce, 0x7c);
const CHARGE: Color32 = Color32::from_rgb(0x7e, 0xb8, 0xd4);
const WARN: Color32 = Color32::from_rgb(0xe2, 0xb1, 0x5a);
const ALARM: Color32 = Color32::from_rgb(0xe0, 0x7a, 0x68);
const CHART_BG: Color32 = Color32::from_rgb(0x12, 0x17, 0x14);
const HOVER_BORDER: Color32 = Color32::from_rgb(0x6d, 0x85, 0x74);
const DISABLED: Color32 = Color32::from_rgb(0x66, 0x70, 0x68);
const DETAIL_BG: Color32 = Color32::from_rgb(0x14, 0x19, 0x15);

struct Series {
    key: &'static str,
    title: &'static str,
    unit: &'static str,
    color: Color32,
    whole: bool,
    location: &'static str,
    text: &'static str,
}

const SERIES: [Series; 11] = [
    Series { key: "packVoltage", title: "Pack voltage", unit: "V", color: GOOD, whole: false, location: "0x181 bytes 4–5", text: "Pack voltage, unit 0.1 V. Shown in volts." },
    Series { key: "current", title: "Current", unit: "A", color: CHARGE, whole: false, location: "0x189 bytes 0–1", text: "Pack current, unit 10 mA. Shown in amperes. Negative is discharge, current drawn from the pack. Positive is charge or regeneration." },
    Series { key: "soc", title: "State of charge", unit: "%", color: WARN, whole: true, location: "0x181 byte 1", text: "State of charge, percent." },
    Series { key: "tempA", title: "Temperature A", unit: "°C", color: ALARM, whole: false, location: "0x189 bytes 2–3", text: "Probe temperature, unit 0.01 °C." },
    Series { key: "tempB", title: "Temperature B", unit: "°C", color: Color32::from_rgb(0xd0, 0x9a, 0x8a), whole: false, location: "0x189 bytes 4–5", text: "Probe temperature, unit 0.01 °C." },
    Series { key: "tempC", title: "Temperature C", unit: "°C", color: Color32::from_rgb(0xc4, 0x7c, 0x9a), whole: false, location: "0x189 bytes 6–7", text: "Probe temperature, unit 0.01 °C." },
    Series { key: "speed", title: "Speed", unit: "km/h", color: Color32::from_rgb(0xc9, 0xd6, 0xa3), whole: true, location: "0x300 byte 0", text: "Road speed." },
    Series { key: "range", title: "Remaining range", unit: "km", color: Color32::from_rgb(0x8f, 0xbf, 0xa8), whole: true, location: "0x300 byte 1", text: "Remaining range. This reading has not been checked against the scooter's own display." },
    Series { key: "rpm", title: "Motor speed", unit: "rpm", color: Color32::from_rgb(0xd4, 0xc0, 0x7a), whole: true, location: "0x391 bytes 0–3", text: "Motor speed. A negative figure appeared while the scooter was in reverse." },
    Series { key: "motor", title: "Motor temperature", unit: "°C", color: Color32::from_rgb(0xc9, 0x8b, 0x78), whole: false, location: "0x301 bytes 0–1", text: "Motor temperature, unit 0.1 °C." },
    Series { key: "inverter", title: "Inverter temperature", unit: "°C", color: Color32::from_rgb(0xb7, 0xa0, 0xc4), whole: false, location: "0x301 bytes 2–3", text: "Inverter temperature, unit 0.1 °C." },
];

const UNIT_ORDER: [&str; 7] = ["V", "A", "%", "°C", "km/h", "km", "rpm"];
const LIVE_BUILTINS: [(&str, &str); 5] = [("pack", "Pack"), ("ride", "Ride"), ("charge", "Charge"), ("drive", "Drive"), ("all", "All")];
const READING_BUILTINS: [(&str, &str); 5] = [("pack", "Pack"), ("cells", "Cells"), ("scooter", "Scooter"), ("motor", "Motor"), ("all", "All")];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    Idle,
    Sim,
    Can,
    Replay,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Live,
    Readings,
    Flags,
    Frames,
    Explore,
    Parameters,
    Events,
    /// The unit's SD card, only when the unit has one.
    SdCard,
    /// The phone's fifth tab, which leads to Parameters and the event log.
    More,
}

/// How much room the window has. The layout follows this, on a computer and in
/// a browser alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Phone,
    Medium,
    Wide,
}

impl Form {
    fn for_width(width: f32) -> Self {
        if width < PHONE_BELOW {
            Form::Phone
        } else if width < WIDE_FROM {
            Form::Medium
        } else {
            Form::Wide
        }
    }
}

/// Whether a tab of the phone's bottom bar is lit. "More" also covers the two
/// screens it leads to.
fn tab_lit(current: Screen, tab: Screen) -> bool {
    tab == current || (tab == Screen::More && matches!(current, Screen::Explore | Screen::Parameters | Screen::Events | Screen::SdCard))
}

struct Detail {
    kicker: String,
    title: String,
    value: String,
    location: String,
    text: String,
}

pub struct MonitorApp {
    snapshot: Arc<Mutex<Snapshot>>,
    stop: Arc<AtomicBool>,
    worker: Option<source::Worker>,
    link: Link,
    screen: Screen,
    nav_collapsed: bool,
    /// The form the window had last frame, to notice when it changes.
    form: Option<Form>,
    phone_seen: bool,
    /// On the phone, whether the Parameters screen shows the ECU side.
    param_ecu: bool,
    ports: Vec<String>,
    selected_port: Option<String>,
    ports_at: Option<Instant>,
    detail: Option<Detail>,
    notice: String,
    notice_bad: bool,
    live_preset: String,
    live_name: String,
    live_on: Vec<bool>,
    live_custom: Vec<NamedView>,
    reading_preset: String,
    reading_name: String,
    reading_on: Vec<bool>,
    reading_custom: Vec<NamedView>,
    /// The signals saved on the Explore screen.
    signal_defs: Vec<SignalDef>,
    /// The Explore screen's own state.
    explore: explore::ExploreState,
    /// The unit's SD card screen: what the unit said and what is on screen.
    sd: sdscreen::SdScreen,
    /// The confirmation to set the battery clock is open.
    confirm_clock: bool,
    /// When the battery clock was last set from here, so it is not set twice at once.
    clock_set_at: Option<Instant>,
    /// A saved view the person asked to delete, waiting for them to confirm:
    /// the screen it is on, its id and its name.
    confirm_delete: Option<(Group, String, String)>,
    /// What the last view action said, and whether it was a problem.
    view_note: Option<(String, bool)>,
    /// Changes to the saved views that have not been answered yet.
    view_pending: u32,
    window_min: u32,
    split: bool,
    hover: String,
    chart_note: String,
    chart_width: f32,
    save_chart: bool,
    pending_chart: Option<(u32, u32, Vec<u8>)>,
    battery_values: Option<Vec<String>>,
    ecu_values: Option<Vec<String>>,
    param_note: String,
    events_on: bool,
    event_note: String,
    event_rows: Vec<stored::EventRow>,
    ask_tx: Option<mpsc::Sender<Ask>>,
    read_busy: bool,
    mark_text: String,
    mark_note: String,
    record_note: String,
    only_undecoded: bool,
    last_replay: Option<Again>,
    /// How far the step buttons and the arrow keys move a replay.
    replay_step_ms: u64,
    /// Recordings a browser still holds from a page that closed before they were saved.
    recoverable: Vec<platform::Recoverable>,
    /// Frames and markers of a recording being saved, to say so when the browser answers.
    pending_save: Option<(u64, u32)>,
    export_note: Option<(Screen, String)>,
    log_note: String,
    log_interval_ms: u64,
    battery_at: Option<u64>,
    ecu_at: Option<u64>,
    baseline: Option<Baseline>,
}

impl MonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);
        let mut app = Self {
            snapshot: Arc::new(Mutex::new(Snapshot::default())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            link: Link::Idle,
            screen: Screen::Live,
            nav_collapsed: false,
            form: None,
            phone_seen: false,
            param_ecu: false,
            ports: Vec::new(),
            selected_port: None,
            ports_at: None,
            detail: None,
            notice: "Disconnected.".into(),
            notice_bad: false,
            live_preset: "pack".into(),
            live_name: String::new(),
            live_on: vec![true; SERIES.len()],
            live_custom: Vec::new(),
            reading_preset: "all".into(),
            reading_name: String::new(),
            reading_on: vec![true; readings::rows().len()],
            reading_custom: Vec::new(),
            signal_defs: Vec::new(),
            explore: explore::ExploreState::default(),
            sd: sdscreen::SdScreen::default(),
            confirm_clock: false,
            clock_set_at: None,
            confirm_delete: None,
            view_note: None,
            view_pending: 0,
            window_min: 1,
            split: false,
            hover: String::new(),
            chart_note: String::new(),
            chart_width: 0.0,
            save_chart: false,
            pending_chart: None,
            battery_values: None,
            ecu_values: None,
            param_note: String::new(),
            events_on: false,
            event_note: String::new(),
            event_rows: Vec::new(),
            ask_tx: None,
            read_busy: false,
            mark_text: String::new(),
            mark_note: String::new(),
            record_note: String::new(),
            only_undecoded: false,
            last_replay: None,
            replay_step_ms: 100,
            recoverable: Vec::new(),
            pending_save: None,
            export_note: None,
            log_note: String::new(),
            log_interval_ms: 1000,
            battery_at: None,
            ecu_at: None,
            baseline: None,
        };
        app.refresh_ports();
        views::load();
        // A browser may still hold a recording from a page that closed before it was saved.
        platform::refresh_recoverable();
        app
    }

    fn refresh_ports(&mut self) {
        let due = self
            .ports_at
            .is_none_or(|then| then.elapsed() >= Duration::from_secs(2));
        if !due {
            return;
        }
        self.ports_at = Some(Instant::now());
        self.ports = slcan::ports();
        if let Some(selected) = &self.selected_port {
            if !self.ports.iter().any(|port| port == selected) {
                self.selected_port = None;
            }
        }
        if self.selected_port.is_none() {
            self.selected_port = self
                .ports
                .iter()
                .find(|port| {
                    port.contains("usbmodem")
                        || port.contains("usbserial")
                        || port.contains("ttyACM")
                        || port.contains("cu.")
                })
                .cloned()
                .or_else(|| self.ports.first().cloned());
        }
    }

    fn halt(&mut self) {
        self.stop_recording();
        self.stop_logging();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.stop = Arc::new(AtomicBool::new(false));
        *lock(&self.snapshot) = Snapshot::default();
        self.link = Link::Idle;
        self.detail = None;
        self.battery_values = None;
        self.ecu_values = None;
        self.battery_at = None;
        self.ecu_at = None;
        self.param_note.clear();
        self.events_on = false;
        self.event_note.clear();
        self.event_rows.clear();
        self.ask_tx = None;
        self.read_busy = false;
        self.hover.clear();
    }

    fn start(&mut self, kind: Kind) {
        self.halt();
        self.notice_bad = false;
        self.notice = match &kind {
            Kind::Simulation => "Simulation. Not a live bus.".into(),
            Kind::Canable(port) => format!("Connected · {port}"),
            Kind::Replay(path) => format!("Replay · {} · not a live bus", file_name(path)),
            Kind::ReplayData(name, _) => format!("Replay · {name} · not a live bus"),
        };
        self.link = match &kind {
            Kind::Simulation => Link::Sim,
            Kind::Canable(_) => Link::Can,
            Kind::Replay(_) | Kind::ReplayData(..) => Link::Replay,
        };
        let ask = if matches!(kind, Kind::Canable(_)) {
            let (tx, rx) = mpsc::channel();
            self.ask_tx = Some(tx);
            Some(rx)
        } else {
            None
        };
        self.worker = Some(source::spawn(
            kind,
            Arc::clone(&self.stop),
            Arc::clone(&self.snapshot),
            ask,
        ));
    }

    fn take_error(&mut self) {
        let error = lock(&self.snapshot).error.clone();
        if let Some(error) = error {
            self.halt();
            self.notice = error;
            self.notice_bad = true;
        }
    }

    /// Notices a live feed that has stopped on its own. A feed ends by itself
    /// only with an error, which it leaves for `take_error`, so a thread that is
    /// gone without one has crashed. Without this the window kept saying
    /// "Connected" over numbers that no longer moved, and a release build on
    /// Windows has no console to show the crash. A browser has no thread to
    /// watch (a crash there stops the whole page).
    #[cfg(not(target_arch = "wasm32"))]
    fn check_worker(&mut self) {
        if !self.worker.as_ref().is_some_and(|worker| worker.is_finished()) {
            return;
        }
        // A feed that ended with an error has said so; let that message show.
        self.take_error();
        let Some(worker) = self.worker.take() else {
            return;
        };
        let reason = match worker.join() {
            Err(payload) => payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "an internal error".to_string()),
            Ok(()) => "it ended without saying why".to_string(),
        };
        self.halt();
        self.notice = format!("The connection stopped unexpectedly ({reason}). Press Connect to start again.");
        self.notice_bad = true;
    }

    #[cfg(target_arch = "wasm32")]
    fn check_worker(&mut self) {}

    fn take_shot(&mut self) {
        let Some((width, height, rgba)) = self.pending_chart.take() else {
            return;
        };
        if width == 0 || height == 0 || rgba.is_empty() {
            self.chart_note = "Chart was not saved.".into();
            return;
        }
        let bytes = crate::png::encode_png(width, height, &rgba);
        self.chart_note = match platform::save("chart.png", "image/png", "PNG image", &["png"], &bytes) {
            Ok(Some(place)) => format!("Chart saved to {place}"),
            Ok(None) => "Chart was not saved.".into(),
            Err(err) => err,
        };
    }
}

impl eframe::App for MonitorApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_ports();
        self.take_error();
        self.check_worker();
        self.take_opened();
        self.take_views();
        self.sd_tick();
        self.sync_signals();
        let wait = if self.link == Link::Idle && self.view_pending == 0 { 400 } else { 80 };
        ctx.request_repaint_after(Duration::from_millis(wait));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.take_error();
        self.take_shot();
        self.take_saved();
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::M)) {
            self.drop_mark();
        }
        let snap = lock(&self.snapshot).clone();
        self.sync_stored(&snap);
        self.apply_form(Form::for_width(ui.ctx().content_rect().width()));
        let phone = self.phone();
        let detail_open = self.detail.is_some();
        let (title_w, controls_w) = self.header_widths(ui, &snap);
        let main_if_docked = ui.available_width() - self.nav_width() - 320.0 - 36.0;
        let dock_detail = !phone && detail_open && main_if_docked >= title_w + 18.0 + controls_w;

        if phone {
            Panel::bottom("tabs")
                .exact_size(TAB_BAR_HEIGHT)
                .resizable(false)
                .show_separator_line(true)
                .frame(panel_frame(PANEL, 4))
                .show(ui, |ui| self.tab_bar(ui));
        } else {
            Panel::left("nav")
                .exact_size(self.nav_width())
                .resizable(false)
                .show_separator_line(true)
                .frame(panel_frame(BG, if self.nav_collapsed { 6 } else { 10 }))
                .show(ui, |ui| self.nav(ui));
        }

        if dock_detail {
            Panel::right("detail")
                .exact_size(320.0)
                .resizable(false)
                .show_separator_line(true)
                .frame(panel_frame(DETAIL_BG, 18))
                .show(ui, |ui| self.detail_panel(ui));
        }

        egui::CentralPanel::default()
            .frame(panel_frame(BG, if phone { 10 } else { 18 }))
            .show(ui, |ui| {
                if phone {
                    self.header_phone(ui, &snap);
                } else {
                    self.header(ui, &snap);
                }
                ui.add_space(8.0);
                if self.link == Link::Replay && self.transport_bar(ui, &snap) {
                    ui.add_space(8.0);
                }
                if self.recover_row(ui) {
                    ui.add_space(8.0);
                }
                let idle_live = self.link == Link::Idle && self.screen == Screen::Live;
                if idle_live {
                    empty_state(ui);
                } else {
                    // The scroll bar floats over the content, so keep a gutter on
                    // the right for it. Without one it covers the units column.
                    ui.spacing_mut().scroll.floating_allocated_width = SCROLL_GUTTER;
                    let column_width = ui.available_width() - SCROLL_GUTTER;
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(column_width);
                            ui.set_max_width(column_width);
                            match self.screen {
                            Screen::Live => self.live(ui, &snap),
                            Screen::Readings => self.readings(ui, &snap),
                            Screen::Flags => {
                                self.flags_toolbar(ui, &snap);
                                flags_page(ui, &snap, &mut self.detail);
                            }
                            Screen::Frames => self.frames(ui, &snap),
                            Screen::Explore => self.explore(ui, &snap),
                            Screen::Parameters => self.parameters(ui, &snap),
                            Screen::Events => self.events(ui, &snap),
                            Screen::SdCard => self.sd_card(ui),
                            Screen::More => self.more_page(ui),
                        }
                        });
                }
                if self.save_chart {
                    self.save_chart = false;
                    let rows = snapshot::in_window(&snap.history, u64::from(self.window_min) * 60_000);
                    let indexes = chart_indexes(&self.live_preset, &self.live_custom, &self.live_on);
                    let image = render_chart_image(
                        ui.ctx(),
                        self.chart_width,
                        self.split,
                        self.window_min,
                        &rows,
                        &indexes,
                        &self.chart_note,
                        &snap.explorer.marks,
                    );
                    if image.0 == 0 || image.1 == 0 {
                        self.chart_note = "Chart was not saved.".into();
                    } else {
                        self.pending_chart = Some(image);
                    }
                }
                if detail_open && !dock_detail {
                    if phone {
                        self.detail_sheet(ui);
                    } else {
                        self.detail_overlay(ui);
                    }
                }
            });
        self.delete_dialog(ui.ctx());
        self.sd_dialog(ui.ctx());
        self.signal_dialog(ui.ctx());
        self.clock_dialog(ui.ctx());
    }
}

impl MonitorApp {
    fn nav_width(&self) -> f32 {
        if self.phone() {
            0.0
        } else if self.nav_collapsed {
            NAV_COLLAPSED_WIDTH
        } else {
            NAV_WIDTH
        }
    }

    fn phone(&self) -> bool {
        self.form == Some(Form::Phone)
    }

    /// Notes the form of the window. A change of form changes what the menu
    /// starts as: collapsed when the room is middling, open when it is wide.
    fn apply_form(&mut self, form: Form) {
        if self.form == Some(form) {
            return;
        }
        self.form = Some(form);
        match form {
            Form::Wide => self.nav_collapsed = false,
            Form::Medium => self.nav_collapsed = true,
            Form::Phone => {
                // A phone has little room for the long list of identifiers.
                if !self.phone_seen {
                    self.phone_seen = true;
                    self.only_undecoded = true;
                }
            }
        }
        if form != Form::Phone && self.screen == Screen::More {
            self.screen = Screen::Live;
        }
    }

    /// The bar along the bottom of a phone, in place of the left menu.
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let tabs = [
            ("Live", Screen::Live),
            ("Readings", Screen::Readings),
            ("Flags", Screen::Flags),
            ("Frames", Screen::Frames),
            ("More", Screen::More),
        ];
        let width = ui.available_width() / tabs.len() as f32;
        let height = ui.available_height();
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.horizontal(|ui| {
            for (label, screen) in tabs {
                let lit = tab_lit(self.screen, screen);
                let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
                let color = if lit { GOOD } else { MUTED };
                if lit {
                    let bar = egui::Rect::from_center_size(
                        egui::pos2(rect.center().x, rect.top() + 1.5),
                        Vec2::new(28.0, 3.0),
                    );
                    ui.painter().rect_filled(bar, CornerRadius::same(2), GOOD);
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(14.0),
                    color,
                );
                if response.clicked() {
                    self.screen = screen;
                    self.view_note = None;
                }
            }
        });
    }

    /// Where the phone's "More" tab leads.
    fn more_page(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("More").size(18.0).color(INK));
        ui.add_space(8.0);
        let mut entries = vec![
            ("Explore", Screen::Explore),
            ("Parameters", Screen::Parameters),
            ("Event log", Screen::Events),
        ];
        if self.sd.available {
            entries.push(("SD card", Screen::SdCard));
        }
        for (label, screen) in entries {
            if nav_button(ui, label, false).clicked() {
                self.screen = screen;
            }
            ui.add_space(6.0);
        }
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(8.0);
        if self.nav_collapsed {
            ui.vertical_centered(|ui| {
                if chevron_button(ui, false).on_hover_text("Show menu").clicked() {
                    self.nav_collapsed = false;
                }
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("S01").color(GOOD).size(13.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if chevron_button(ui, true).on_hover_text("Hide menu").clicked() {
                    self.nav_collapsed = true;
                }
            });
        });
        ui.add_space(12.0);
        let mut entries = vec![
            ("Live", Screen::Live),
            ("Readings", Screen::Readings),
            ("Flags", Screen::Flags),
            ("Frames", Screen::Frames),
            ("Explore", Screen::Explore),
            ("Parameters", Screen::Parameters),
            ("Event log", Screen::Events),
        ];
        if self.sd.available {
            entries.push(("SD card", Screen::SdCard));
        }
        for (step, (label, screen)) in entries.into_iter().enumerate() {
            if step > 0 {
                ui.add_space(6.0);
            }
            if nav_button(ui, label, self.screen == screen).clicked() {
                self.screen = screen;
                self.view_note = None;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(RichText::new(format!("v{VERSION}")).size(11.0).color(MUTED));
            });
        });
    }

    fn header_widths(&self, ui: &egui::Ui, snap: &Snapshot) -> (f32, f32) {
        let title = text_width(ui, "Silence S01", 22.0).max(text_width(ui, &self.notice, 13.0));
        let pack = pack_label(snap.pack_id.as_deref());
        let clock = snap.clock.as_deref().unwrap_or("—");
        let gap = 8.0;
        let controls = button_width(ui, &pack)
            + gap
            + text_width(ui, clock, 15.0)
            + gap
            + 220.0
            + gap
            + button_width(ui, "Simulate")
            + gap
            + button_width(ui, "Connect")
            + gap
            + button_width(ui, "Disconnect");
        (title, controls)
    }

    fn header_title(&self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(RichText::new("Silence S01").size(22.0).color(INK));
            let color = if self.notice_bad {
                ALARM
            } else if self.link == Link::Sim || self.link == Link::Replay {
                GOOD
            } else {
                MUTED
            };
            ui.label(RichText::new(&self.notice).size(13.0).color(color));
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let (title_w, controls_w) = self.header_widths(ui, snap);
        if title_w + 18.0 + controls_w <= ui.available_width() {
            ui.horizontal(|ui| {
                self.header_title(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.header_controls(ui, snap, false);
                });
            });
        } else {
            self.header_title(ui);
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                self.header_controls(ui, snap, true);
            });
        }
    }

    fn header_controls(&mut self, ui: &mut egui::Ui, snap: &Snapshot, left_to_right: bool) {
        let port_width = 220.0_f32.min(ui.available_size_before_wrap().x.max(1.0));
        if left_to_right {
            self.header_pack(ui, snap);
            self.header_clock(ui, snap);
            self.header_port(ui, port_width);
            self.header_simulate(ui);
            self.header_connect(ui);
            self.header_disconnect(ui);
        } else {
            self.header_disconnect(ui);
            self.header_connect(ui);
            self.header_simulate(ui);
            self.header_port(ui, port_width);
            self.header_clock(ui, snap);
            self.header_pack(ui, snap);
        }
    }

    fn header_pack(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let pack = pack_label(snap.pack_id.as_deref());
        let mut button = ui.add(Button::new(pack));
        if let Some(id) = snap.pack_id.as_deref() {
            button = button.on_hover_text(format!("Pack ID {id}"));
        }
        if button.clicked() {
            let hex = snap
                .pack_id
                .as_deref()
                .and_then(|id| id.parse::<u32>().ok())
                .map(|value| format!(" In hex that is {value:08X}."))
                .unwrap_or_default();
            self.detail = Some(Detail {
                kicker: "Battery".into(),
                title: "Pack ID".into(),
                value: snap.pack_id.clone().unwrap_or_default(),
                location: "0x183 bytes 0–3. Also parameter index 0x21.".into(),
                text: format!(
                    "Battery pack ID, shown as a decimal number.{hex} 4294967295 (0xFFFFFFFF) means unprogrammed."
                ),
            });
        }
    }

    fn header_clock(&self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.label(RichText::new(snap.clock.as_deref().unwrap_or("—")).color(MUTED).size(15.0));
    }

    fn header_port(&mut self, ui: &mut egui::Ui, width: f32) {
        let label = self.selected_port.clone().unwrap_or_else(|| NO_PORT.into());
        egui::ComboBox::from_id_salt("port")
            .width(width)
            .selected_text(label)
            .show_ui(ui, |ui| {
                if self.ports.is_empty() {
                    ui.label(RichText::new(NO_PORT).color(MUTED));
                }
                for port in self.ports.clone() {
                    ui.selectable_value(&mut self.selected_port, Some(port.clone()), port);
                }
            });
    }

    fn header_simulate(&mut self, ui: &mut egui::Ui) {
        if action_button(ui, "Simulate", self.link == Link::Idle).clicked() {
            self.start(Kind::Simulation);
        }
    }

    fn header_connect(&mut self, ui: &mut egui::Ui) {
        if action_button(ui, "Connect", self.link == Link::Idle).clicked() {
            match self.selected_port.clone() {
                Some(port) => self.start(Kind::Canable(port)),
                None => {
                    self.notice = format!("Choose a {PORT_WORD}.");
                    self.notice_bad = true;
                }
            }
        }
    }

    fn header_disconnect(&mut self, ui: &mut egui::Ui) {
        if action_button(ui, "Disconnect", self.link != Link::Idle).clicked() {
            self.halt();
            self.screen = Screen::Live;
            self.notice = "Disconnected.".into();
            self.notice_bad = false;
        }
    }

    fn detail_overlay(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().content_rect();
        let main_width = (screen.right() - screen.left() - self.nav_width()).max(0.0);
        let width = 320.0_f32.min(main_width);
        let rect = egui::Rect::from_min_max(egui::pos2(screen.right() - width, screen.top()), screen.right_bottom());
        egui::Area::new(egui::Id::unique("detail-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                let bounds = ui.max_rect();
                ui.painter().rect_filled(bounds, CornerRadius::ZERO, DETAIL_BG);
                ui.painter().line_segment(
                    [bounds.left_top(), bounds.left_bottom()],
                    Stroke::new(1.0, LINE_COLOR),
                );
                let inner = bounds.shrink(18.0);
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)),
                    |ui| self.detail_panel(ui),
                );
            });
    }

    /// The header of a phone: the title and status, one connect button, and a
    /// menu with the rest.
    fn header_phone(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let color = if self.notice_bad {
            ALARM
        } else if self.link == Link::Sim || self.link == Link::Replay {
            GOOD
        } else {
            MUTED
        };
        let notice = self.notice.clone();
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("…", |ui| self.header_menu(ui, snap));
                if self.link == Link::Idle {
                    self.header_connect(ui);
                } else {
                    self.header_disconnect(ui);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Silence S01").size(18.0).color(INK));
                        ui.add(egui::Label::new(RichText::new(notice).size(12.0).color(color)).truncate());
                    });
                });
            });
        });
    }

    /// What the phone's header keeps out of sight: the pack, the clock, the
    /// serial port on a computer, and the simulation.
    fn header_menu(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.set_min_width(200.0);
        self.header_pack(ui, snap);
        self.header_clock(ui, snap);
        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.separator();
            ui.label(RichText::new("Serial port").size(12.0).color(MUTED));
            if self.ports.is_empty() {
                ui.label(RichText::new(NO_PORT).color(MUTED));
            }
            for port in self.ports.clone() {
                ui.selectable_value(&mut self.selected_port, Some(port.clone()), port);
            }
        }
        ui.separator();
        self.header_simulate(ui);
        ui.separator();
        ui.label(RichText::new(format!("Version {VERSION}")).size(12.0).color(MUTED));
    }

    /// On a phone the detail comes up from the bottom, over the tab bar.
    fn detail_sheet(&mut self, ui: &mut egui::Ui) {
        let screen = ui.ctx().content_rect();
        let height = (screen.height() * 0.6).min(420.0);
        let rect = egui::Rect::from_min_max(egui::pos2(screen.left(), screen.bottom() - height), screen.right_bottom());
        egui::Area::new(egui::Id::unique("detail-sheet"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .constrain(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                let bounds = ui.max_rect();
                let top = CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 };
                ui.painter().rect_filled(bounds, top, DETAIL_BG);
                ui.painter().line_segment(
                    [bounds.left_top(), bounds.right_top()],
                    Stroke::new(1.0, LINE_COLOR),
                );
                let inner = bounds.shrink(18.0);
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)),
                    |ui| self.detail_panel(ui),
                );
            });
    }

    fn live(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        self.view_bar(ui);
        ui.add_space(10.0);
        self.heroes(ui, snap);
        ui.add_space(12.0);
        self.chart(ui, snap);
        ui.add_space(12.0);
        let phone = self.phone();
        cells(ui, snap, &mut self.detail, phone);
        ui.add_space(12.0);
        if set_flags(ui, snap, &mut self.detail) {
            self.screen = Screen::Flags;
        }
        ui.add_space(12.0);
    }

    fn view_bar(&mut self, ui: &mut egui::Ui) {
        if self.phone() {
            self.view_chips(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("View").size(13.0).color(MUTED));
            let label = live_label(&self.live_preset, &self.live_custom);
            egui::ComboBox::from_id_salt("live-view")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (id, name) in live_choices(&self.live_custom) {
                        ui.selectable_value(&mut self.live_preset, id, name);
                    }
                });
            self.view_tail(ui);
        });
        self.show_view_note(ui);
    }

    /// On a phone the views are a row of chips, one tap each, that scrolls
    /// sideways when there are more than fit.
    fn view_chips(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::horizontal()
            .id_salt("live-view-chips")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (id, name) in live_choices(&self.live_custom) {
                        let lit = self.live_preset == id;
                        if chip(ui, &name, lit).clicked() {
                            self.live_preset = id;
                        }
                    }
                });
            });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| self.view_tail(ui));
        self.show_view_note(ui);
    }

    /// The name box and Save view while composing, or Delete view on a saved
    /// view.
    fn view_tail(&mut self, ui: &mut egui::Ui) {
        if self.live_preset == "all" {
            ui.add(
                TextEdit::singleline(&mut self.live_name)
                    .hint_text("View name")
                    .desired_width(160.0),
            );
            if action_button(ui, "Save view", self.view_pending == 0).clicked() {
                self.save_live();
            }
        } else if let Some(view) = self.live_custom.iter().find(|view| view.id == self.live_preset) {
            let (id, name) = (view.id.clone(), view.name.clone());
            if action_button(ui, "Delete view", self.view_pending == 0).clicked() {
                self.confirm_delete = Some((Group::Live, id, name));
            }
        }
    }

    fn show_view_note(&self, ui: &mut egui::Ui) {
        if let Some((text, bad)) = &self.view_note {
            ui.label(RichText::new(text).size(12.0).color(if *bad { ALARM } else { GOOD }));
        }
    }

    fn save_live(&mut self) {
        let selected = self.live_on.iter().filter(|on| **on).count();
        if let Some(problem) = view_problem(&self.live_name, &live_used(&self.live_custom), selected) {
            self.view_note = Some((problem.into(), true));
            return;
        }
        let keys = SERIES
            .iter()
            .enumerate()
            .filter(|(index, _)| self.live_on[*index])
            .map(|(_, series)| series.key.to_string())
            .collect();
        self.view_note = None;
        self.view_pending += 1;
        views::change(views::Change::Add {
            group: Group::Live,
            name: self.live_name.clone(),
            keys,
            reserved: LIVE_BUILTINS.iter().map(|(_, name)| (*name).to_string()).collect(),
        });
    }

    /// Takes in what the saved views said since the last frame.
    fn take_views(&mut self) {
        for outcome in views::take_outcomes() {
            if !outcome.load {
                self.view_pending = self.view_pending.saturating_sub(1);
            }
            let mut saved = outcome.saved.unwrap_or_else(|| Saved {
                live: std::mem::take(&mut self.live_custom),
                reading: std::mem::take(&mut self.reading_custom),
                signals: std::mem::take(&mut self.signal_defs),
            });
            let mut selected = outcome.selected;
            let mut error = outcome.error;
            if let Some(change) = &outcome.unsaved {
                match views::apply(&mut saved, change) {
                    Ok(chosen) => selected = chosen,
                    Err(problem) => error = Some(problem),
                }
            }
            self.live_custom = saved.live;
            self.reading_custom = saved.reading;
            self.signal_defs = saved.signals;
            if !live_known(&self.live_preset, &self.live_custom) {
                self.live_preset = "pack".into();
            }
            if !reading_known(&self.reading_preset, &self.reading_custom) {
                self.reading_preset = "all".into();
            }
            match selected {
                Some((Group::Live, id)) => {
                    self.live_preset = id;
                    self.live_name.clear();
                    self.live_on = vec![true; SERIES.len()];
                }
                Some((Group::Reading, id)) => {
                    self.reading_preset = id;
                    self.reading_name.clear();
                    self.reading_on.fill(true);
                }
                None => {}
            }
            // A failed load at start-up needs no message; the next save says
            // if the views cannot be kept.
            if !outcome.load || outcome.unsaved.is_some() {
                if let Some(error) = error {
                    self.view_note = Some((error, true));
                }
            }
        }
    }

    /// The battery's clock next to this device's, and the button that sets it.
    /// Setting it is the one thing the app writes to the scooter.
    fn clock_card(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let local = clock::local_now();
        let cooling = self
            .clock_set_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(CLOCK_COOLDOWN_S));
        let can_set = self.link == Link::Can && !self.read_busy && !snap.read_busy && local.is_some() && !cooling;
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Battery clock").size(16.0).color(INK));
                if action_button(ui, "Set battery clock…", can_set).clicked() {
                    self.confirm_clock = true;
                }
            });
            ui.add_space(4.0);
            let battery = snap.battery_clock.map_or_else(|| "—".to_string(), |moment| moment.text());
            let device = local.map_or_else(|| "—".to_string(), |moment| moment.text());
            egui::Grid::new("battery-clock").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Battery").color(MUTED));
                ui.label(RichText::new(battery).monospace().color(INK));
                ui.end_row();
                ui.label(RichText::new("This device").color(MUTED));
                ui.label(RichText::new(device).monospace().color(INK));
                ui.end_row();
            });
            if let (Some(battery), Some(device)) = (snap.battery_clock, local) {
                let offset = battery.seconds() - device.seconds();
                let text = match offset {
                    -2..=2 => "The same, to within 2 s".to_string(),
                    1.. => format!("{} ahead", clock::span_text(offset)),
                    _ => format!("{} behind", clock::span_text(offset)),
                };
                let far = offset.abs() > CLOCK_FAR_S;
                ui.label(RichText::new(format!("Difference: {text}")).color(if far { WARN } else { MUTED }));
                if far {
                    ui.label(RichText::new(CLOCK_FAR).size(12.0).color(WARN));
                }
            }
            ui.add_space(2.0);
            let hint = match self.link {
                Link::Idle => "Connect an adapter to set the battery clock.",
                Link::Sim => "A simulation has no battery to set.",
                Link::Replay => "A replay has no scooter to set.",
                Link::Can => CLOCK_HINT,
            };
            ui.label(RichText::new(hint).size(12.0).color(MUTED));
            if !snap.clock_note.is_empty() {
                let good = snap.clock_note == clock::NOTE_MATCHES;
                ui.label(RichText::new(&snap.clock_note).color(if good { GOOD } else { WARN }));
            }
        });
    }

    /// Asks before the battery clock is set, and sets it on a yes. The time is
    /// taken again at the moment of the yes, so the seconds are right.
    fn clock_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_clock {
            return;
        }
        let shown = clock::local_now();
        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::unique("set-clock")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new("Set the battery clock?").size(18.0).color(INK));
            ui.add_space(6.0);
            let text = match shown {
                Some(moment) => format!("It will be set to this device's time, now {}.", moment.text()),
                None => clock::NOTE_NO_TIME.to_string(),
            };
            ui.label(RichText::new(text).color(INK));
            ui.add_space(4.0);
            ui.label(RichText::new(CLOCK_HINT).color(MUTED));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if action_button(ui, "Set the clock", shown.is_some()).clicked() {
                    confirmed = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        });
        if confirmed {
            self.confirm_clock = false;
            self.send_clock();
        } else if cancelled || modal.should_close() {
            self.confirm_clock = false;
        }
    }

    /// Sends the request to set the battery clock to this device's time now.
    fn send_clock(&mut self) {
        let note = match (clock::local_now(), &self.ask_tx) {
            (Some(moment), Some(tx)) if self.link == Link::Can => {
                if tx.send(Ask::SetClock(moment)).is_ok() {
                    self.clock_set_at = Some(Instant::now());
                    clock::NOTE_SETTING
                } else {
                    "The battery clock could not be set: the connection is not working."
                }
            }
            (None, _) => clock::NOTE_NO_TIME,
            _ => "Connect an adapter to set the battery clock.",
        };
        lock(&self.snapshot).clock_note = note.into();
    }

    /// Asks before a saved view is deleted, and deletes it on a yes.
    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some((group, id, name)) = self.confirm_delete.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::unique("delete-view")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new("Delete this view?").size(18.0).color(INK));
            ui.add_space(6.0);
            ui.label(RichText::new(format!("\"{name}\" will be removed. {DELETE_WARNING}")).color(MUTED));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.add(Button::new(RichText::new("Delete").color(ALARM))).clicked() {
                    confirmed = true;
                }
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        });
        if confirmed {
            self.confirm_delete = None;
            self.view_note = None;
            self.view_pending += 1;
            views::change(views::Change::Remove { group, id });
        } else if cancelled || modal.should_close() {
            self.confirm_delete = None;
        }
    }

    fn heroes(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let (composing, tiles) = live_indexes(&self.live_preset, &self.live_custom);
        let gap = 10.0;
        let min_tile = 140.0;
        let width = ui.available_width().max(min_tile);
        let columns = ((width + gap) / (min_tile + gap)).floor().max(1.0) as usize;
        let tile_w = (width - gap * (columns.saturating_sub(1) as f32)) / columns as f32;
        let row_gap = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = gap;
        for row in tiles.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for index in row {
                    let index = *index;
                    let off = composing && !self.live_on[index];
                    let series = &SERIES[index];
                    let response = ui
                        .scope(|ui| {
                            ui.set_width(tile_w);
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                if off {
                                    ui.set_opacity(0.38);
                                }
                                card(ui, |ui| {
                                    ui.label(RichText::new(series.title).size(12.0).color(MUTED));
                                    let value = sample_field(&snap.sample_now(), series.key);
                                    let text = value
                                        .map(|number| format_series(series, number))
                                        .unwrap_or_else(|| "—".into());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(text).size(26.0).color(INK));
                                        ui.label(RichText::new(series.unit).size(12.0).color(MUTED));
                                    });
                                    let start = snap.history.len().saturating_sub(90);
                                    let values: Vec<f64> = snap
                                        .history
                                        .iter()
                                        .skip(start)
                                        .filter_map(|sample| {
                                            sample_field(sample, series.key).map(f64::from)
                                        })
                                        .collect();
                                    sparkline(ui, &values, series.color, if self.phone() { 26.0 } else { 36.0 });
                                });
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if response.clicked() {
                        if composing {
                            self.live_on[index] = !self.live_on[index];
                        } else {
                            self.detail = Some(series_detail(
                                series,
                                sample_field(&snap.sample_now(), series.key),
                            ));
                        }
                    }
                }
            });
        }
        ui.spacing_mut().item_spacing.y = row_gap;
    }

    fn chart(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let rows = snapshot::in_window(&snap.history, u64::from(self.window_min) * 60_000);
        let indexes = chart_indexes(&self.live_preset, &self.live_custom, &self.live_on);
        let note = self.chart_note.clone();
        let split = self.split;
        let window_min = self.window_min;
        let phone = self.phone();
        let heights = if phone { CHART_HEIGHTS_PHONE } else { CHART_HEIGHTS };
        let scope = ui.scope(|ui| {
            card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                    ui.label(RichText::new("Stream").size(16.0).color(INK));
                    if phone {
                        // The window is a row of chips; the rest is in a menu.
                        for minutes in [1, 5, 10, 15] {
                            if chip(ui, &format!("{minutes}m"), window_min == minutes).clicked() {
                                self.window_min = minutes;
                            }
                        }
                        ui.menu_button("…", |ui| {
                            if ui.button(if split { "Overlay" } else { "Split" }).clicked() {
                                self.split = !self.split;
                            }
                            if ui.button("Reset").clicked() {
                                lock(&self.snapshot).history.clear();
                            }
                            if ui.button("Save chart").clicked() {
                                self.save_chart = true;
                            }
                            if action_button(ui, "Export data…", !snap.history.is_empty()).clicked() {
                                self.export_history(snap);
                            }
                        });
                        return;
                    }
                    egui::ComboBox::from_id_salt("chart-window")
                        .selected_text(format!("{window_min} min"))
                        .show_ui(ui, |ui| {
                            for minutes in [1, 5, 10, 15] {
                                ui.selectable_value(&mut self.window_min, minutes, format!("{minutes} min"));
                            }
                        });
                    let mut split_button = Button::new(if split { "Overlay" } else { "Split" });
                    if split {
                        split_button = split_button.stroke(Stroke::new(1.0, INK));
                    }
                    if ui.add(split_button).clicked() {
                        self.split = !self.split;
                    }
                    if ui.button("Reset").clicked() {
                        lock(&self.snapshot).history.clear();
                    }
                    if ui.button("Save chart").clicked() {
                        self.save_chart = true;
                    }
                    if action_button(ui, "Export data…", !snap.history.is_empty()).clicked() {
                        self.export_history(snap);
                    }
                });
                self.show_export_note(ui);
                ui.add_space(8.0);
                let split_now = self.split;
                self.hover = chart_graphs(ui, split_now, &rows, &indexes, &note, &snap.explorer.marks, heights);
            });
        });
        self.chart_width = scope.response.rect.width();
    }

    fn readings(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let (composing, keys) = reading_keys(&self.reading_preset, &self.reading_custom);
        control_row(ui, self.phone(), |ui| {
            ui.label(RichText::new("View").size(13.0).color(MUTED));
            let label = reading_label(&self.reading_preset, &self.reading_custom);
            egui::ComboBox::from_id_salt("reading-view")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (id, name) in reading_choices(&self.reading_custom) {
                        ui.selectable_value(&mut self.reading_preset, id, name);
                    }
                });
            if composing {
                ui.add(TextEdit::singleline(&mut self.reading_name).hint_text("View name").desired_width(160.0));
                let all_on = self.reading_on.iter().all(|on| *on);
                if ui.button(if all_on { "Deselect all" } else { "Select all" }).clicked() {
                    self.reading_on.fill(!all_on);
                }
                if action_button(ui, "Save view", self.view_pending == 0).clicked() {
                    self.save_reading();
                }
            } else if let Some(view) = self.reading_custom.iter().find(|view| view.id == self.reading_preset) {
                let (id, name) = (view.id.clone(), view.name.clone());
                if action_button(ui, "Delete view", self.view_pending == 0).clicked() {
                    self.confirm_delete = Some((Group::Reading, id, name));
                }
            }
            if action_button(ui, "Export readings…", snap.has_signal()).clicked() {
                self.export_readings(snap, &keys, composing);
            }
        });
        self.show_view_note(ui);
        self.show_export_note(ui);
        ui.add_space(8.0);
        for group in ["Pack", "Cells", "Scooter", "Motor"] {
            let rows: Vec<&Row> = readings::rows()
                .iter()
                .filter(|row| row.group == group && keys.iter().any(|key| *key == row.key))
                .collect();
            if rows.is_empty() {
                continue;
            }
            ui.add_space(8.0);
            ui.label(RichText::new(group.to_uppercase()).size(13.0).color(MUTED));
            for row in rows {
                let index = readings::rows().iter().position(|item| item.key == row.key).unwrap_or(0);
                let off = composing && !self.reading_on[index];
                let response = ui
                    .scope(|ui| {
                        if off {
                            ui.set_opacity(0.38);
                        }
                        param_row(ui, row.label, &readings::figure(snap, row), row.unit, 76.0)
                    })
                    .inner;
                if response.clicked() {
                    if composing {
                        self.reading_on[index] = !self.reading_on[index];
                    } else {
                        self.detail = Some(Detail {
                            kicker: row.group.into(),
                            title: row.label.into(),
                            value: readings::shown(snap, row),
                            location: row.location.into(),
                            text: row.text.into(),
                        });
                    }
                }
            }
        }
    }

    fn save_reading(&mut self) {
        let selected = self.reading_on.iter().filter(|on| **on).count();
        if let Some(problem) = view_problem(&self.reading_name, &reading_used(&self.reading_custom), selected) {
            self.view_note = Some((problem.into(), true));
            return;
        }
        let keys = readings::rows()
            .iter()
            .enumerate()
            .filter(|(index, _)| self.reading_on[*index])
            .map(|(_, row)| row.key.to_string())
            .collect();
        self.view_note = None;
        self.view_pending += 1;
        views::change(views::Change::Add {
            group: Group::Reading,
            name: self.reading_name.clone(),
            keys,
            reserved: READING_BUILTINS.iter().map(|(_, name)| (*name).to_string()).collect(),
        });
    }

    fn sync_stored(&mut self, snap: &Snapshot) {
        if self.link != Link::Can {
            return;
        }
        self.read_busy = snap.read_busy;
        if !snap.param_note.is_empty() {
            self.param_note = snap.param_note.clone();
        }
        if !snap.battery_values.is_empty() {
            self.battery_values = Some(snap.battery_values.clone());
            self.battery_at = snap.battery_read_ms;
        }
        if !snap.ecu_values.is_empty() {
            self.ecu_values = Some(snap.ecu_values.clone());
            self.ecu_at = snap.ecu_read_ms;
        }
        if !snap.event_note.is_empty() {
            self.event_note = snap.event_note.clone();
        }
        if snap.events_ready {
            self.event_rows = snap.event_rows.clone();
            self.events_on = !snap.event_rows.is_empty();
        }
    }

    fn read_failed(&mut self, ask: Ask) {
        let note = "Connect an adapter before reading.";
        if ask == Ask::Events {
            self.events_on = false;
            self.event_note = note.into();
        } else {
            self.param_note = note.into();
        }
    }

    fn request_read(&mut self, ask: Ask) {
        match self.link {
            Link::Idle | Link::Replay => {
                let note = if self.link == Link::Replay {
                    "A replay has no scooter to read from. Disconnect, then connect an adapter."
                } else {
                    "Connect an adapter before reading."
                };
                if ask == Ask::Events {
                    self.events_on = false;
                    self.event_note = note.into();
                } else {
                    self.param_note = note.into();
                }
            }
            Link::Sim => {
                if ask == Ask::Events {
                    let count = params::EVENTS.len();
                    self.event_rows = params::EVENTS
                        .iter()
                        .enumerate()
                        .map(|(place, row)| {
                            [
                                row[0].into(),
                                row[1].into(),
                                row[2].into(),
                                row[3].into(),
                                (count - place).to_string(),
                                String::new(),
                            ]
                        })
                        .collect();
                    self.events_on = true;
                    self.event_note = "Sample event log.".into();
                } else {
                    let sections = if ask == Ask::Battery { params::battery() } else { params::ecu() };
                    let values = params::fill(sections);
                    if ask == Ask::Battery {
                        self.battery_values = Some(values);
                        self.battery_at = Some(snapshot::now_ms());
                    } else {
                        self.ecu_values = Some(values);
                        self.ecu_at = Some(snapshot::now_ms());
                    }
                    self.param_note = "Sample parameter values.".into();
                }
            }
            Link::Can => {
                if self.read_busy {
                    // Say so, instead of leaving the button looking dead. The
                    // event log already says on its own screen that it is reading.
                    if ask != Ask::Events {
                        lock(&self.snapshot).param_note = "Another read is still running. Try again in a moment.".into();
                    }
                    return;
                }
                let Some(tx) = &self.ask_tx else {
                    self.read_failed(ask);
                    return;
                };
                if tx.send(ask).is_err() {
                    self.read_failed(ask);
                    return;
                }
                self.read_busy = true;
                {
                    let mut snap = lock(&self.snapshot);
                    snap.read_busy = true;
                    let note = match ask {
                        Ask::Battery => "Reading battery parameters.",
                        Ask::Ecu => "Reading ECU parameters.",
                        Ask::Events => "Reading the event log.",
                        // Not a read: it goes through `confirm_clock_dialog`.
                        Ask::SetClock(_) => return,
                    };
                    if ask == Ask::Events {
                        snap.event_note = note.into();
                        snap.events_ready = false;
                        self.event_note = note.into();
                    } else {
                        snap.param_note = note.into();
                        self.param_note = note.into();
                    }
                }
            }
        }
    }

    /// The parameters read so far, as rows, battery first.
    fn current_params(&self) -> Vec<ParamRow> {
        let stamp = |at: Option<u64>| at.map_or_else(String::new, export::stamp);
        let mut rows = paramdiff::param_rows(
            "Battery",
            params::battery(),
            self.battery_values.as_deref(),
            &stamp(self.battery_at),
        );
        rows.extend(paramdiff::param_rows(
            "ECU",
            params::ecu(),
            self.ecu_values.as_deref(),
            &stamp(self.ecu_at),
        ));
        rows
    }

    fn load_baseline(&mut self) {
        platform::open("baseline", "CSV", &["csv"]);
    }

    /// Files the person picked since the last frame.
    fn take_opened(&mut self) {
        for opened in platform::take_opened() {
            match opened.tag {
                "baseline" => {
                    let loaded = opened
                        .bytes
                        .and_then(|bytes| paramdiff::parse_param_file(&String::from_utf8_lossy(&bytes)));
                    match loaded {
                        Ok(rows) => {
                            self.baseline = Some(Baseline {
                                label: format!("file {}", opened.name),
                                rows,
                            });
                            self.export_note = None;
                        }
                        Err(err) => self.say_export(err),
                    }
                }
                "replay" => match opened.bytes {
                    Ok(bytes) => {
                        let again = match opened.path {
                            Some(path) => Again::Path(path),
                            None => Again::Data(opened.name, bytes),
                        };
                        self.begin_replay(again);
                    }
                    Err(err) => {
                        self.notice = err;
                        self.notice_bad = true;
                    }
                },
                _ => {}
            }
        }
    }

    fn parameters(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        self.clock_card(ui, snap);
        ui.add_space(10.0);
        let current = self.current_params();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Read from Battery").clicked() {
                self.request_read(Ask::Battery);
            }
            if ui.button("Read from ECU").clicked() {
                self.request_read(Ask::Ecu);
            }
            if action_button(ui, "Export parameters…", !current.is_empty()).clicked() {
                let table = paramdiff::param_table(&current, &self.origin(snap));
                self.save_table("parameters", &table);
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if action_button(ui, "Set baseline", !current.is_empty()).clicked() {
                let now = export::stamp(snapshot::now_ms());
                self.baseline = Some(Baseline {
                    label: format!("reads taken this session, set at {} UTC", &now[11..19]),
                    rows: current.clone(),
                });
                self.export_note = None;
            }
            if ui.button("Compare with file…").clicked() {
                self.load_baseline();
            }
            if action_button(ui, "Clear baseline", self.baseline.is_some()).clicked() {
                self.baseline = None;
            }
        });
        if !self.param_note.is_empty() {
            ui.label(RichText::new(&self.param_note).color(MUTED));
        }
        self.show_export_note(ui);
        let (changes, compared) = match &self.baseline {
            Some(baseline) => paramdiff::compare(&baseline.rows, &current),
            None => (Vec::new(), 0),
        };
        self.changes_panel(ui, &changes, compared, !current.is_empty());
        ui.add_space(8.0);
        let flags = |device: &str, count: usize| {
            let mut flags = vec![false; count];
            for change in changes.iter().filter(|change| change.device == device) {
                if let Some(flag) = flags.get_mut(change.index) {
                    *flag = true;
                }
            }
            flags
        };
        let battery_flags = flags("Battery", params::len(params::battery()));
        let ecu_flags = flags("ECU", params::len(params::ecu()));
        if self.phone() {
            // One side at a time, chosen with chips, instead of two columns.
            ui.horizontal(|ui| {
                if chip(ui, "Battery", !self.param_ecu).clicked() {
                    self.param_ecu = false;
                }
                if chip(ui, "ECU", self.param_ecu).clicked() {
                    self.param_ecu = true;
                }
            });
            ui.add_space(6.0);
            if self.param_ecu {
                param_sections(ui, params::ecu(), self.ecu_values.as_deref(), &ecu_flags, &mut self.detail);
            } else {
                param_sections(ui, params::battery(), self.battery_values.as_deref(), &battery_flags, &mut self.detail);
            }
            return;
        }
        let gap = 22.0;
        let column = ((ui.available_width() - gap) * 0.5).floor().max(0.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.vertical(|ui| {
                ui.set_min_width(column);
                ui.set_max_width(column);
                ui.label(RichText::new("Battery").size(18.0).color(INK));
                param_sections(ui, params::battery(), self.battery_values.as_deref(), &battery_flags, &mut self.detail);
            });
            ui.vertical(|ui| {
                ui.set_min_width(column);
                ui.set_max_width(column);
                ui.label(RichText::new("ECU").size(18.0).color(INK));
                param_sections(ui, params::ecu(), self.ecu_values.as_deref(), &ecu_flags, &mut self.detail);
            });
        });
    }

    fn changes_panel(&self, ui: &mut egui::Ui, changes: &[Change], compared: usize, have_reads: bool) {
        ui.add_space(8.0);
        let Some(baseline) = &self.baseline else {
            ui.label(
                RichText::new("To see what changed: read, press Set baseline, change something, then read again. Or compare with a saved parameter export.")
                    .size(12.0)
                    .color(MUTED),
            );
            return;
        };
        ui.label(RichText::new(format!("Baseline: {}", baseline.label)).color(MUTED));
        if !have_reads {
            ui.label(RichText::new("Read the battery or the ECU to compare it with the baseline.").color(MUTED));
        } else if compared == 0 {
            ui.label(
                RichText::new("Nothing to compare: the baseline and the current reads have no parameter in common.")
                    .color(WARN),
            );
        } else if changes.is_empty() {
            ui.label(RichText::new(format!("No differences in {compared} values.")).color(GOOD));
        } else {
            ui.label(
                RichText::new(format!("{} changed of {compared} values", changes.len()))
                    .size(16.0)
                    .color(WARN),
            );
            ui.add_space(4.0);
            if self.phone() {
                for change in changes {
                    ui.label(RichText::new(format!("{} · {} · {}", change.device, change.section, change.name)).color(INK));
                    ui.label(
                        RichText::new(format!("{} → {} {}", change.before, change.after, change.unit)).color(WARN),
                    );
                    ui.add_space(4.0);
                }
                return;
            }
            egui::Grid::new("param-changes").num_columns(6).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                for heading in ["Device", "Section", "Parameter", "Before", "Now", "Unit"] {
                    ui.label(RichText::new(heading).color(MUTED));
                }
                ui.end_row();
                for change in changes {
                    ui.label(RichText::new(&change.device).color(INK));
                    ui.label(RichText::new(&change.section).color(MUTED));
                    ui.label(RichText::new(&change.name).color(INK));
                    ui.label(RichText::new(&change.before).color(INK));
                    ui.label(RichText::new(&change.after).color(WARN));
                    ui.label(RichText::new(&change.unit).color(MUTED));
                    ui.end_row();
                }
            });
        }
    }

    fn events(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        control_row(ui, self.phone(), |ui| {
            if ui.button("Read Event Logs").clicked() {
                self.request_read(Ask::Events);
            }
            if action_button(ui, "Save Logs", self.events_on).clicked() {
                let mut text = String::new();
                for row in &self.event_rows {
                    text.push_str(&stored::event_row_line(row));
                    text.push('\n');
                }
                match platform::save("events.txt", "text/plain", "Text", &["txt"], text.as_bytes()) {
                    Ok(Some(place)) => self.event_note = format!("Log saved to {place}"),
                    Ok(None) => {}
                    Err(err) => self.event_note = err,
                }
            }
            if action_button(ui, "Export CSV…", self.events_on).clicked() {
                let table = paramdiff::event_table(&self.event_rows, &self.origin(snap));
                self.save_table("events", &table);
            }
            if ui.button("Reset").clicked() {
                self.events_on = false;
                self.event_note.clear();
                self.event_rows.clear();
                let mut snap = lock(&self.snapshot);
                snap.event_rows.clear();
                snap.event_note.clear();
                snap.events_ready = false;
            }
        });
        if !self.event_note.is_empty() {
            ui.label(RichText::new(&self.event_note).color(MUTED));
        }
        self.show_export_note(ui);
        ui.label(RichText::new(EVENT_READ_NOTE).size(12.0).color(MUTED));
        ui.add_space(8.0);
        if !self.events_on {
            ui.label(RichText::new("No events.").color(MUTED));
            return;
        }
        egui::Grid::new("events").num_columns(5).spacing([16.0, 6.0]).show(ui, |ui| {
            for heading in ["No.", "Time", "Code", "Data", "Text"] {
                ui.label(RichText::new(heading).color(MUTED));
            }
            ui.end_row();
            // The record number comes last in a row but first on the screen. The
            // raw bytes are only in the saved log and the export.
            for row in &self.event_rows {
                for column in [4, 0, 1, 2, 3] {
                    ui.label(RichText::new(&row[column]).color(INK));
                }
                ui.end_row();
            }
        });
    }

    fn detail_panel(&mut self, ui: &mut egui::Ui) {
        if ui.add(Button::new("Close")).clicked() {
            self.detail = None;
            return;
        }
        let Some(detail) = &self.detail else {
            return;
        };
        ui.add_space(12.0);
        ui.label(RichText::new(&detail.kicker).size(12.0).color(GOOD));
        ui.label(RichText::new(&detail.title).size(22.0).color(INK));
        if !detail.value.is_empty() {
            ui.label(RichText::new(&detail.value).size(28.0).color(INK));
        }
        ui.add_space(8.0);
        ui.label(RichText::new(&detail.location).color(MUTED));
        ui.label(RichText::new(&detail.text).color(MUTED));
    }
}

impl MonitorApp {
    fn drop_mark(&mut self) {
        let card = self.unit_recording();
        if self.link == Link::Idle && !card {
            self.mark_note = "Connect, simulate or replay first, then mark.".into();
            return;
        }
        let typed = self.mark_text.trim().to_string();
        let text = {
            let mut snap = lock(&self.snapshot);
            let text = if typed.is_empty() {
                format!("Mark {}", snap.explorer.marks.len() + 1)
            } else {
                typed
            };
            if self.link != Link::Idle {
                snap.mark_now(&text);
            }
            text
        };
        // A recording the unit is making to its card gets the marker too.
        if card {
            sdcard::send(sdcard::mark(&text));
        }
        self.mark_note = format!("Marked: {text}");
        self.mark_text.clear();
    }

    fn start_recording(&mut self) {
        let now = snapshot::now_ms();
        let name = format!("recording-{}.log", capture::file_stamp(now));
        let source = match (self.link, &self.selected_port) {
            (Link::Can, Some(port)) => port.clone(),
            (Link::Replay, _) => "replay".to_string(),
            _ => "unknown".to_string(),
        };
        // A computer writes to the file as frames arrive. A browser keeps the
        // recording in its own storage as it goes (in memory if it has none) and
        // downloads it when recording stops.
        #[cfg(not(target_arch = "wasm32"))]
        let made = {
            let Some(path) = rfd::FileDialog::new().set_file_name(&name).save_file() else {
                return;
            };
            Recorder::create(&path, &source, now)
        };
        #[cfg(target_arch = "wasm32")]
        let made = match platform::chunk_store() {
            Some(store) => Recorder::chunked(&name, &source, now, store),
            None => Recorder::in_memory(&name, &source, now),
        };
        match made {
            Ok(recorder) => {
                lock(&self.snapshot).recorder = Some(Arc::new(Mutex::new(recorder)));
                self.record_note.clear();
            }
            Err(err) => self.record_note = format!("Could not create the log file: {err}"),
        }
    }

    fn stop_recording(&mut self) {
        let recorder = lock(&self.snapshot).recorder.take();
        let Some(recorder) = recorder else {
            return;
        };
        let mut rec = recorder.lock().unwrap_or_else(|poison| poison.into_inner());
        rec.finish();
        // A browser keeps a recording in its own storage as it goes. Building the
        // file and downloading it takes a moment; the browser answers through
        // `take_saved`.
        if let Some((id, tail)) = rec.chunk_tail() {
            let name = rec.path.display().to_string();
            platform::save_chunks(&id, &name, tail);
            self.pending_save = Some((rec.frames, rec.marks));
            self.record_note = "Saving the recording to your downloads...".into();
            return;
        }
        let place = match rec.take_bytes() {
            Some(bytes) => {
                let name = rec.path.display().to_string();
                match platform::save(&name, "text/plain", "Capture log", &["log"], &bytes) {
                    Ok(Some(place)) => place,
                    Ok(None) => "nowhere (not saved)".to_string(),
                    Err(err) => err,
                }
            }
            None => rec.path.display().to_string(),
        };
        self.record_note = match &rec.error {
            Some(err) => err.clone(),
            None => format!("Saved {} frames and {} markers to {place}", rec.frames, rec.marks),
        };
    }

    /// What the browser said about a recording it was asked to save.
    fn take_saved(&mut self) {
        self.show_saved(platform::take_saved());
        self.recoverable = platform::recoverable();
    }

    fn show_saved(&mut self, messages: Vec<String>) {
        for message in messages {
            self.record_note = match self.pending_save.take() {
                Some((frames, marks)) => format!("Saved {frames} frames and {marks} markers to {message}"),
                None => format!("Recording saved to {message}"),
            };
        }
    }

    /// Recordings a browser kept from a page that closed before they were saved,
    /// with a button to download each and one to delete it. Drawn on every
    /// screen until they are dealt with. Returns true when it drew anything.
    fn recover_row(&mut self, ui: &mut egui::Ui) -> bool {
        if self.recoverable.is_empty() {
            return false;
        }
        for item in self.recoverable.clone() {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "A recording was left in this browser when the page closed: {} ({}). The last few seconds may be missing.",
                        item.name,
                        size_text(item.bytes)
                    ))
                    .color(WARN),
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Download it").clicked() {
                        platform::recover_save(&item.id, &item.name);
                        self.record_note = "Preparing the recording...".into();
                    }
                    if ui.button("Delete it").clicked() {
                        platform::recover_discard(&item.id);
                        self.recoverable.retain(|other| other.id != item.id);
                    }
                });
            });
        }
        true
    }

    fn pick_replay(&mut self) {
        platform::open("replay", "Capture logs", &["log", "txt", "asc", "csv"]);
    }

    fn begin_replay(&mut self, again: Again) {
        let kind = match &again {
            Again::Path(path) => Kind::Replay(path.clone()),
            Again::Data(name, bytes) => Kind::ReplayData(name.clone(), bytes.clone()),
        };
        self.last_replay = Some(again);
        self.record_note.clear();
        self.mark_note.clear();
        self.start(kind);
    }

    /// Where the data in an export came from, written into every row.
    fn origin(&self, snap: &Snapshot) -> String {
        match self.link {
            Link::Sim => "Simulation, not a bus".into(),
            Link::Replay => format!(
                "Replay of {}",
                snap.replay.as_ref().map_or("a capture", |state| state.name.as_str())
            ),
            Link::Can => match &self.selected_port {
                Some(port) => format!("Live bus ({port})"),
                None => "Live bus".into(),
            },
            Link::Idle => "No source".into(),
        }
    }

    fn csv_name(&self, kind: &str) -> String {
        let tag = match self.link {
            Link::Sim => "simulation",
            Link::Replay => "replay",
            Link::Can => "live",
            Link::Idle => "none",
        };
        format!("s01-{kind}-{tag}-{}.csv", capture::file_stamp(snapshot::now_ms()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ask_csv_path(&self, kind: &str) -> Option<PathBuf> {
        let mut path = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .set_file_name(self.csv_name(kind))
            .save_file()?;
        if path.extension().is_none() {
            path.set_extension("csv");
        }
        Some(path)
    }

    fn say_export(&mut self, text: String) {
        self.export_note = Some((self.screen, text));
    }

    fn show_export_note(&self, ui: &mut egui::Ui) {
        if let Some((screen, text)) = &self.export_note {
            if *screen == self.screen {
                ui.label(RichText::new(text).size(12.0).color(GOOD));
            }
        }
    }

    fn save_table(&mut self, kind: &str, table: &Table) {
        let name = self.csv_name(kind);
        let bytes = export::table_bytes(table);
        let note = match platform::save(&name, "text/csv", "CSV", &["csv"], &bytes) {
            Ok(Some(place)) => format!("Saved {} rows to {place}", table.rows.len()),
            Ok(None) => return,
            Err(err) => err,
        };
        self.say_export(note);
    }

    fn export_readings(&mut self, snap: &Snapshot, keys: &[&'static str], composing: bool) {
        let rows: Vec<&Row> = readings::rows()
            .iter()
            .enumerate()
            .filter(|(index, row)| keys.contains(&row.key) && (!composing || self.reading_on[*index]))
            .map(|(_, row)| row)
            .collect();
        let table = export::readings_table(snap, &rows, &self.origin(snap));
        self.save_table("readings", &table);
    }

    fn flags_toolbar(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let any = explain::catalogue().iter().any(|flag| snap.bytes_for(flag.id).is_some());
        ui.horizontal(|ui| {
            if action_button(ui, "Export flags…", any).clicked() {
                let table = export::flag_table(snap, &self.origin(snap));
                self.save_table("flags", &table);
            }
        });
        self.show_export_note(ui);
        ui.add_space(6.0);
        self.debug_switch(ui, snap);
    }

    /// The person's debug mode switch, on the Frames and Flags screens. Off at
    /// every start, so the app only listens until it is switched on. The session
    /// does the sending (`slcan.rs`); this only sets the switch and says what
    /// is going on.
    fn debug_switch(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let live = self.link == Link::Can;
        ui.horizontal_wrapped(|ui| {
            let (label, color) = if snap.debug_wanted && live { ("Debug mode: on", GOOD) } else { ("Debug mode: off", INK) };
            let button = Button::new(RichText::new(label).color(if live { color } else { DISABLED }));
            let button = if live { button } else { button.sense(Sense::hover()) };
            if ui.add(button).on_hover_text("Switch the scooter's ECU debug mode on or off").clicked() {
                lock(&self.snapshot).debug_wanted = !snap.debug_wanted;
            }
            let hint = if live {
                DEBUG_HINT
            } else {
                "Connect to a scooter to use the debug mode switch. Until you switch it on, the app only listens."
            };
            ui.label(RichText::new(hint).size(12.0).color(MUTED));
        });
        if live {
            if let Some(note) = debug_note(snap.debug_wanted, snap.debug) {
                ui.label(RichText::new(note).size(12.0).color(MUTED));
            }
        }
    }

    fn export_history(&mut self, snap: &Snapshot) {
        let table = export::history_table(snap, &self.origin(snap));
        self.save_table("history", &table);
    }

    fn start_logging(&mut self, snap: &Snapshot) {
        #[cfg(not(target_arch = "wasm32"))]
        let made = {
            let Some(path) = self.ask_csv_path("values") else {
                return;
            };
            ValueLogger::create(&path, &self.origin(snap), self.log_interval_ms)
        };
        #[cfg(target_arch = "wasm32")]
        let made = ValueLogger::in_memory(&self.csv_name("values"), &self.origin(snap), self.log_interval_ms);
        match made {
            Ok(logger) => {
                lock(&self.snapshot).logger = Some(Arc::new(Mutex::new(logger)));
                self.log_note.clear();
            }
            Err(err) => self.log_note = format!("Could not create the log file: {err}"),
        }
    }

    fn stop_logging(&mut self) {
        let logger = lock(&self.snapshot).logger.take();
        let Some(logger) = logger else {
            return;
        };
        let mut log = logger.lock().unwrap_or_else(|poison| poison.into_inner());
        log.finish();
        let place = match log.take_bytes() {
            Some(bytes) => {
                let name = log.path.display().to_string();
                match platform::save(&name, "text/csv", "CSV", &["csv"], &bytes) {
                    Ok(Some(place)) => place,
                    Ok(None) => "nowhere (not saved)".to_string(),
                    Err(err) => err,
                }
            }
            None => log.path.display().to_string(),
        };
        self.log_note = match &log.error {
            Some(err) => err.clone(),
            None => format!("Saved {} rows to {place}", log.rows),
        };
    }

    fn log_row(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.horizontal_wrapped(|ui| {
            if snap.logger.is_some() {
                if ui.button("Stop logging").clicked() {
                    self.stop_logging();
                }
            } else {
                if action_button(ui, "Log values", self.link != Link::Idle).clicked() {
                    self.start_logging(snap);
                }
                egui::ComboBox::from_id_salt("log-interval")
                    .selected_text(interval_label(self.log_interval_ms))
                    .show_ui(ui, |ui| {
                        for choice in [250_u64, 1000, 5000] {
                            ui.selectable_value(&mut self.log_interval_ms, choice, interval_label(choice));
                        }
                    });
            }
            if let Some((path, rows, error)) = snap.logger_info() {
                if let Some(error) = error {
                    ui.label(RichText::new(error).color(ALARM));
                } else {
                    ui.label(
                        RichText::new(format!(
                            "Logging · {rows} rows · {}",
                            path.rsplit(['/', '\\']).next().unwrap_or(&path)
                        ))
                        .color(GOOD),
                    );
                }
            } else if !self.log_note.is_empty() {
                ui.label(RichText::new(&self.log_note).color(MUTED));
            } else if self.link == Link::Idle {
                ui.label(RichText::new("Connect, simulate or replay, then log every reading to a spreadsheet file.").color(MUTED));
            }
        });
    }

    fn export_row(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.horizontal_wrapped(|ui| {
            if action_button(ui, "Export frames…", !snap.explorer.ids.is_empty()).clicked() {
                let table = export::frame_list(snap, &self.origin(snap));
                self.save_table("frames", &table);
            }
            if action_button(ui, "Export markers…", !snap.explorer.marks.is_empty()).clicked() {
                let table = export::marker_list(snap, &self.origin(snap));
                self.save_table("markers", &table);
            }
        });
        self.show_export_note(ui);
    }

    fn frames(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        self.record_row(ui, snap);
        ui.add_space(8.0);
        self.debug_switch(ui, snap);
        ui.add_space(8.0);
        self.replay_row(ui, snap);
        ui.add_space(8.0);
        self.mark_row(ui);
        ui.add_space(8.0);
        self.log_row(ui, snap);
        ui.add_space(8.0);
        self.export_row(ui, snap);
        ui.add_space(14.0);
        marks_table(ui, snap);
        ui.add_space(14.0);
        let phone = self.phone();
        frames_table(ui, snap, &mut self.only_undecoded, phone);
        ui.add_space(12.0);
    }

    fn record_row(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        // One Record button. On the page of a unit with a card that is ready, and
        // connected to that unit's bus, it records on the card; otherwise on this
        // device, as before. A recording the unit is already making (started in
        // another browser, or by itself at power-up) is stopped from here too.
        let to_card = self.record_to_card();
        let unit_recording = self.unit_recording();
        ui.horizontal_wrapped(|ui| {
            let can = matches!(self.link, Link::Can | Link::Replay);
            if snap.recorder.is_some() {
                if ui.button("Stop recording").clicked() {
                    self.stop_recording();
                }
            } else if unit_recording {
                if ui.button("Stop recording").clicked() {
                    self.stop_card_recording();
                }
            } else if to_card {
                if ui.button("Record to the SD card").clicked() {
                    self.start_card_recording();
                }
            } else if action_button(ui, "Record", can).clicked() {
                self.start_recording();
            }
            if snap.recorder.is_some() && unit_recording {
                self.card_record_line(ui);
            }
            if let Some((path, frames, marks, error)) = snap.recorder_info() {
                if let Some(error) = error {
                    ui.label(RichText::new(error).color(ALARM));
                } else if let Some(warning) = snap.recorder_warning() {
                    ui.label(RichText::new(warning).color(WARN));
                } else {
                    ui.label(
                        RichText::new(format!(
                            "Recording · {frames} frames · {marks} markers · {}",
                            path.rsplit(['/', '\\']).next().unwrap_or(&path)
                        ))
                        .color(GOOD),
                    );
                }
            } else if unit_recording || self.sd.saved.is_some() {
                self.card_record_line(ui);
            } else if !self.record_note.is_empty() {
                ui.label(RichText::new(&self.record_note).color(MUTED));
            } else if self.link == Link::Sim {
                ui.label(RichText::new("The simulation makes no CAN frames, so there is nothing to record.").color(MUTED));
            } else if self.link == Link::Idle {
                ui.label(RichText::new("Connect to the scooter, then record every frame to a log file.").color(MUTED));
            }
        });
    }

    /// The replay controls, drawn under the header on every screen while a
    /// capture is playing. Returns true when it drew anything.
    fn transport_bar(&mut self, ui: &mut egui::Ui, snap: &Snapshot) -> bool {
        let Some(state) = &snap.replay else {
            return false;
        };
        let busy = snap.recorder.is_some() || snap.logger.is_some();
        let can_jump = !busy;
        let typing = ui.ctx().egui_wants_keyboard_input();
        let mut jump_to: Option<u64> = None;
        let mut toggle = false;
        let mut stop = false;
        let mut restart = false;
        let mut from_slider = false;
        let mut speed = state.speed;
        let mut step_ms = self.replay_step_ms;
        // Several clicks before the replay has caught up add up, because each
        // one starts from the jump already asked for.
        let here = state.seek_to.unwrap_or(state.position_ms);
        let step_back = |step: u64| here.saturating_sub(step);
        let step_on = |step: u64| (here + step).min(state.length_ms);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                let play_label = if state.finished {
                    "Play again"
                } else if state.paused {
                    "Play"
                } else {
                    "Pause"
                };
                if action_button(ui, play_label, !state.finished || can_jump).clicked() {
                    toggle = true;
                }
                let back = ui.add_enabled(can_jump && here > 0, Button::new("<"));
                if back.on_hover_text("Step back").clicked() {
                    jump_to = Some(step_back(step_ms));
                }
                let on = ui.add_enabled(can_jump && here < state.length_ms, Button::new(">"));
                if on.on_hover_text("Step forward").clicked() {
                    jump_to = Some(step_on(step_ms));
                }
                egui::ComboBox::from_id_salt("replay-step")
                    .selected_text(step_label(step_ms))
                    .show_ui(ui, |ui| {
                        for choice in REPLAY_STEPS_MS {
                            ui.selectable_value(&mut step_ms, choice, step_label(choice));
                        }
                    })
                    .response
                    .on_hover_text("How far a step moves");
                egui::ComboBox::from_id_salt("replay-speed")
                    .selected_text(format!("{speed}×"))
                    .show_ui(ui, |ui| {
                        for choice in [0.1_f32, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0] {
                            ui.selectable_value(&mut speed, choice, format!("{choice}×"));
                        }
                    })
                    .response
                    .on_hover_text("Playing speed");
                if ui.button("Restart").clicked() {
                    restart = true;
                }
                if ui.button("Stop replay").clicked() {
                    stop = true;
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(position_text(state.position_ms)).monospace().color(INK));
                let mut at = state.position_ms as f64;
                let room = (ui.available_width() - 120.0).max(80.0);
                ui.spacing_mut().slider_width = room;
                let slider = ui.add_enabled(
                    can_jump && state.length_ms > 0,
                    egui::Slider::new(&mut at, 0.0..=state.length_ms as f64).show_value(false),
                );
                if slider.changed() {
                    jump_to = Some(at as u64);
                    from_slider = true;
                }
                ui.label(RichText::new(mmss(state.length_ms)).monospace().color(MUTED));
            });
            let note = if busy {
                "Stop the recording or the values log to jump around. Pause and speed still work.".to_string()
            } else {
                if self.phone() {
                    state.name.clone()
                } else {
                    format!("{} · Space plays or pauses; the arrow keys step.", state.name)
                }
            };
            ui.label(RichText::new(note).size(12.0).color(MUTED));
            if !state.source_note.is_empty() {
                ui.label(RichText::new(&state.source_note).size(12.0).color(WARN));
            }
        });
        if !typing {
            let (space, left, right) = ui.input(|input| {
                (
                    input.key_pressed(egui::Key::Space),
                    input.key_pressed(egui::Key::ArrowLeft),
                    input.key_pressed(egui::Key::ArrowRight),
                )
            });
            toggle |= space && (!state.finished || can_jump);
            if can_jump && left {
                jump_to = Some(step_back(step_ms));
            }
            if can_jump && right && here < state.length_ms {
                jump_to = Some(step_on(step_ms));
            }
        }
        self.replay_step_ms = step_ms;
        if stop {
            self.halt();
            self.notice = "Replay stopped.".into();
            return true;
        }
        if restart {
            if let Some(again) = self.last_replay.clone() {
                self.begin_replay(again);
                return true;
            }
        }
        if toggle || jump_to.is_some() || speed != state.speed {
            if let Some(live) = lock(&self.snapshot).replay.as_mut() {
                live.speed = speed;
                if toggle {
                    if state.finished && can_jump {
                        live.seek_to = Some(0);
                        live.paused = false;
                    } else {
                        live.paused = !live.paused;
                    }
                }
                if let Some(at) = jump_to {
                    // Dragging the slider leaves play and pause as they were. A
                    // step button or key pauses, so the moment holds.
                    live.seek_to = Some(at);
                    if !from_slider {
                        live.paused = true;
                    }
                }
            }
        }
        true
    }

    fn replay_row(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.horizontal_wrapped(|ui| {
            let can = matches!(self.link, Link::Idle | Link::Replay);
            if action_button(ui, "Replay a capture…", can).clicked() {
                self.pick_replay();
            }
            let Some(state) = &snap.replay else {
                if self.link == Link::Replay {
                    ui.label(RichText::new("Loading…").color(MUTED));
                } else if !can {
                    ui.label(RichText::new("Disconnect first to replay a capture.").color(MUTED));
                } else {
                    ui.label(
                        RichText::new("Play a recording or one of the dump logs through the app, with no scooter.")
                            .color(MUTED),
                    );
                }
                return;
            };
            ui.label(RichText::new(&state.name).color(MUTED));
            if self.link == Link::Replay {
                ui.label(RichText::new("The play, step and jump controls are in the bar above, on every screen.").color(MUTED));
            }
        });
    }

    fn mark_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let edit = ui.add(
                TextEdit::singleline(&mut self.mark_text)
                    .hint_text("Note, e.g. pressed reverse")
                    .desired_width(260.0),
            );
            let entered = edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let clicked = action_button(ui, "Mark", self.link != Link::Idle).clicked();
            if clicked || entered {
                self.drop_mark();
            }
            let keys = if cfg!(target_os = "macos") { "Cmd+Shift+M" } else { "Ctrl+Shift+M" };
            ui.label(
                RichText::new(format!("{keys} marks from any screen. Markers go into the log while recording."))
                    .size(12.0)
                    .color(MUTED),
            );
        });
        if !self.mark_note.is_empty() {
            ui.label(RichText::new(&self.mark_note).size(12.0).color(GOOD));
        }
    }
}

fn interval_label(ms: u64) -> &'static str {
    match ms {
        250 => "4 rows a second",
        1000 => "1 row a second",
        _ => "1 row every 5 s",
    }
}

/// A size in bytes for a person: `340 KB`, `12.3 MB`.
fn size_text(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} KB", bytes.div_ceil(1000))
    }
}

fn mmss(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

/// The moments a step can move by.
const REPLAY_STEPS_MS: [u64; 5] = [10, 100, 1000, 10_000, 60_000];

fn step_label(ms: u64) -> String {
    if ms >= 1000 {
        format!("{} s step", ms / 1000)
    } else {
        format!("{ms} ms step")
    }
}

/// Minutes, seconds and tenths, for example `1:07.4`.
fn position_text(ms: u64) -> String {
    format!("{}:{:02}.{}", ms / 60_000, ms / 1000 % 60, ms / 100 % 10)
}

/// Shown beside the debug mode switch.
const DEBUG_HINT: &str = "With it off the app only listens. The ECU's inputs, outputs and status (on the Flags screen) have only been seen with debug mode on, and not on every scooter.";

/// What the app did about debug mode and why; it is not a claim about what
/// debug mode does to the scooter.
fn debug_note(wanted: bool, state: snapshot::DebugState) -> Option<&'static str> {
    use snapshot::DebugState;
    match state {
        DebugState::Idle if wanted => Some("Debug mode is switched on. The app sends it when the scooter is awake."),
        DebugState::Idle => None,
        DebugState::Sent => Some("The app has asked the scooter to switch debug mode on and is waiting for the scooter to report it."),
        DebugState::Confirmed => Some(
            "Debug mode is on and the scooter reports it. The app switches it off when you switch it off here or disconnect.",
        ),
        DebugState::AlreadyOn => Some("Debug mode was already on when the scooter woke, so the app sent nothing and leaves it as it was."),
        DebugState::Unconfirmed => Some(
            "The app asked the scooter to switch debug mode on, twice, but the scooter did not report it as on. This scooter may not report it. The app switches it off when you switch it off here or disconnect.",
        ),
    }
}

/// Lines of CAN_NOTES.md (`0x201`, Wording), shown on the Battery clock card.
const CLOCK_HINT: &str = "Setting the clock sends one message to the battery with this device's date and time, and the battery writes the change in its event log.";
const CLOCK_FAR: &str = "The battery clock is more than a minute away from this device's clock.";
/// The card shows a difference above this many seconds in amber, and the button
/// is held for this many seconds after a set.
const CLOCK_FAR_S: i64 = 60;
const CLOCK_COOLDOWN_S: u64 = 10;
/// A line of CAN_NOTES.md (`0x1ED` / `0x1EE`, Wording), shown under the buttons of
/// the Event log screen.
const EVENT_READ_NOTE: &str = "The log is read ten records at a time, with a short pause between, because reading it all at once can make the motor controller report a fault.";
#[cfg(not(target_arch = "wasm32"))]
const DELETE_WARNING: &str = "This can't be undone.";
#[cfg(target_arch = "wasm32")]
const DELETE_WARNING: &str = "It goes from the device too, so it disappears for every browser. This can't be undone.";
#[cfg(not(target_arch = "wasm32"))]
const NO_PORT: &str = "No serial port";
#[cfg(target_arch = "wasm32")]
const NO_PORT: &str = "No device";
#[cfg(not(target_arch = "wasm32"))]
const PORT_WORD: &str = "serial port";
#[cfg(target_arch = "wasm32")]
const PORT_WORD: &str = "device";
#[cfg(not(target_arch = "wasm32"))]
const CONNECT_HINT: &str = "Connect a CANable, or press Simulate to fill the dashboard with sample data.";
#[cfg(target_arch = "wasm32")]
const CONNECT_HINT: &str =
    "Connect to the device on the scooter, press Simulate, or pick a capture on the Frames screen.";

/// What "Restart" plays again: a file on disk, or a capture held in memory.
#[derive(Clone)]
enum Again {
    Path(PathBuf),
    Data(String, Vec<u8>),
}

/// A computer brings its fonts. The browser build leaves the big default set
/// out to keep the download small and carries just this one.
#[cfg(target_arch = "wasm32")]
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "ubuntu".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("ubuntu.ttf"))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("ubuntu".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[cfg(not(target_arch = "wasm32"))]
fn install_fonts(_ctx: &egui::Context) {}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map_or_else(|| "capture".to_string(), |name| name.to_string_lossy().into_owned())
}

fn marks_table(ui: &mut egui::Ui, snap: &Snapshot) {
    ui.label(RichText::new("Markers").size(16.0).color(INK));
    ui.add_space(4.0);
    let marks = &snap.explorer.marks;
    if marks.is_empty() {
        ui.label(RichText::new("No markers yet.").color(MUTED));
        return;
    }
    let start = snap.explorer.ids.values().map(|stats| stats.first_ms).min().unwrap_or(0);
    egui::Grid::new("marks").num_columns(3).spacing([16.0, 4.0]).show(ui, |ui| {
        for heading in ["#", "After first frame", "Note"] {
            ui.label(RichText::new(heading).color(MUTED));
        }
        ui.end_row();
        for (index, mark) in marks.iter().enumerate() {
            let after = mark.t_ms.saturating_sub(start);
            ui.label(RichText::new(format!("{}", index + 1)).color(WARN));
            ui.label(RichText::new(format!("{}.{}", mmss(after), after / 100 % 10)).color(INK));
            ui.label(RichText::new(&mark.text).color(INK));
            ui.end_row();
        }
    });
}

fn age_text(ms: u64) -> String {
    if ms < 10_000 {
        format!("{:.1} s", ms as f32 / 1000.0)
    } else if ms < 60_000 {
        format!("{} s", ms / 1000)
    } else {
        mmss(ms)
    }
}

fn rate_text(stats: &frames::IdStats) -> String {
    stats.rate().map_or_else(
        || "—".to_string(),
        |rate| if rate >= 10.0 { format!("{rate:.0} Hz") } else { format!("{rate:.1} Hz") },
    )
}

/// `phone` lays the identifiers out as one small card each, which is easier to
/// read on a narrow screen than a table of seven columns.
fn frames_table(ui: &mut egui::Ui, snap: &Snapshot, only_undecoded: &mut bool, phone: bool) {
    let explorer = &snap.explorer;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Identifiers").size(16.0).color(INK));
        ui.label(
            RichText::new(format!(
                "{} seen · {} not decoded · {} frames",
                explorer.ids.len(),
                explorer.undecoded(),
                explorer.total
            ))
            .color(MUTED),
        );
        ui.checkbox(only_undecoded, "Only not decoded");
    });
    ui.add_space(4.0);
    if explorer.ids.is_empty() {
        ui.label(RichText::new("No frames yet.").color(MUTED));
        return;
    }
    ui.label(
        RichText::new("Green bytes changed in the last moment. White bytes have changed at some point. Grey bytes have not moved.")
            .size(12.0)
            .color(MUTED),
    );
    ui.add_space(6.0);
    let now = if snap.replay.is_some() { explorer.now_ms } else { snapshot::now_ms() };
    if phone {
        for (id, stats) in &explorer.ids {
            if *only_undecoded && stats.decoded {
                continue;
            }
            let age = now.saturating_sub(stats.last_ms);
            let quiet = stats.period_ms.is_some_and(|period| age as f32 > (period * 5.0).max(3000.0));
            panel_frame(PANEL, 8)
                .stroke(Stroke::new(1.0, LINE_COLOR))
                .corner_radius(CornerRadius::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("0x{id:03X}")).monospace().color(INK));
                        ui.label(RichText::new(rate_text(stats)).monospace().color(INK));
                        ui.label(RichText::new(format!("{} frames", stats.count)).size(12.0).color(MUTED));
                        ui.label(RichText::new(age_text(age)).monospace().color(if quiet { WARN } else { MUTED }));
                    });
                    ui.label(byte_job(stats, now));
                    if stats.decoded {
                        let name = frames::label(*id);
                        ui.label(RichText::new(if name.is_empty() { "Decoded" } else { name }).color(MUTED));
                    } else {
                        ui.label(RichText::new("Not decoded").color(WARN));
                    }
                });
            ui.add_space(6.0);
        }
        return;
    }
    egui::Grid::new("frames")
        .num_columns(7)
        .spacing([14.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for heading in ["ID", "Frames", "Rate", "DLC", "Data", "Last seen", "Decoding"] {
                ui.label(RichText::new(heading).color(MUTED));
            }
            ui.end_row();
            for (id, stats) in &explorer.ids {
                if *only_undecoded && stats.decoded {
                    continue;
                }
                ui.label(RichText::new(format!("0x{id:03X}")).monospace().color(INK));
                ui.label(RichText::new(stats.count.to_string()).monospace().color(INK));
                let rate = stats.rate().map_or_else(
                    || "—".to_string(),
                    |rate| if rate >= 10.0 { format!("{rate:.0} Hz") } else { format!("{rate:.1} Hz") },
                );
                ui.label(RichText::new(rate).monospace().color(INK));
                ui.label(RichText::new(stats.len.to_string()).monospace().color(INK));
                ui.label(byte_job(stats, now));
                let age = now.saturating_sub(stats.last_ms);
                let quiet = stats.period_ms.is_some_and(|period| age as f32 > (period * 5.0).max(3000.0));
                ui.label(RichText::new(age_text(age)).monospace().color(if quiet { WARN } else { MUTED }));
                if stats.decoded {
                    let name = frames::label(*id);
                    ui.label(RichText::new(if name.is_empty() { "Decoded" } else { name }).color(MUTED));
                } else {
                    ui.label(RichText::new("Not decoded").color(WARN));
                }
                ui.end_row();
            }
        });
}

fn byte_job(stats: &frames::IdStats, now: u64) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    for index in 0..8 {
        let (text, color) = if index < stats.len {
            let color = if stats.changed_recently(index, now) {
                GOOD
            } else if stats.has_varied(index) {
                INK
            } else {
                MUTED
            };
            (format!("{:02X} ", stats.data[index]), color)
        } else {
            ("   ".to_string(), MUTED)
        };
        job.append(
            &text,
            0.0,
            TextFormat {
                font_id: egui::FontId::monospace(13.0),
                color,
                ..Default::default()
            },
        );
    }
    job
}

fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.ctx()
        .fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), egui::FontId::proportional(size), INK).size().x)
}

fn button_width(ui: &egui::Ui, label: &str) -> f32 {
    text_width(ui, label, 15.0) + 26.0
}

fn action_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let color = if enabled { INK } else { DISABLED };
    let mut button = Button::new(RichText::new(label).color(color));
    if !enabled {
        button = button.sense(Sense::hover());
    }
    ui.add(button)
}

/// The arrow that hides and shows the menu. It is drawn, not typed, so it
/// does not depend on the font having an arrow in it.
fn chevron_button(ui: &mut egui::Ui, points_left: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(8), PANEL2);
        ui.painter()
            .rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, HOVER_BORDER), egui::StrokeKind::Inside);
    }
    let color = if response.hovered() { INK } else { MUTED };
    let (cx, cy) = (rect.center().x, rect.center().y);
    let (base, tip) = if points_left { (cx + 3.0, cx - 3.0) } else { (cx - 3.0, cx + 3.0) };
    ui.painter().line(
        vec![egui::pos2(base, cy - 6.0), egui::pos2(tip, cy), egui::pos2(base, cy + 6.0)],
        Stroke::new(2.0, color),
    );
    response
}

/// A row of controls. On a phone it runs onto further lines instead of off the
/// edge of the screen.
fn control_row(ui: &mut egui::Ui, wrap: bool, add_contents: impl FnOnce(&mut egui::Ui)) {
    if wrap {
        ui.horizontal_wrapped(add_contents);
    } else {
        ui.horizontal(add_contents);
    }
}

/// A small rounded choice, lit when it is the one in use.
fn chip(ui: &mut egui::Ui, label: &str, lit: bool) -> egui::Response {
    let (text, fill, border) = if lit {
        (INK, PANEL2, HOVER_BORDER)
    } else {
        (MUTED, Color32::TRANSPARENT, LINE_COLOR)
    };
    ui.add(
        Button::new(RichText::new(label).size(13.0).color(text))
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(100)),
    )
}

fn nav_button(ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 31.0), Sense::click());
    let fill = if active { PANEL } else { Color32::TRANSPARENT };
    let stroke = if active {
        Stroke::new(1.0, LINE_COLOR)
    } else if response.hovered() {
        Stroke::new(1.0, HOVER_BORDER)
    } else {
        Stroke::NONE
    };
    let color = if active { INK } else { MUTED };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    if stroke != Stroke::NONE {
        ui.painter().rect_stroke(rect, CornerRadius::same(8), stroke, egui::StrokeKind::Inside);
    }
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(15.0),
        color,
    );
    response
}

fn empty_state(ui: &mut egui::Ui) {
    ui.add_space((ui.available_height() * 0.28).clamp(48.0, 220.0));
    ui.vertical_centered(|ui| {
        // Never wider than the room, or the hint runs off the edge of a phone.
        ui.set_max_width(ui.available_width().min(460.0));
        ui.label(RichText::new("No stream").size(22.0).color(INK));
        ui.add_space(6.0);
        ui.label(
            RichText::new(CONNECT_HINT).color(MUTED),
        );
    });
}

/// Why a view cannot be saved yet, in words for the person.
fn view_problem(name: &str, used: &[String], selected: usize) -> Option<&'static str> {
    if readings::can_save(name, used, selected) {
        None
    } else if name.trim().is_empty() {
        Some("Give the view a name.")
    } else if selected == 0 {
        Some("Turn at least one item on.")
    } else {
        Some("A view with that name already exists.")
    }
}

fn live_known(preset: &str, custom: &[NamedView]) -> bool {
    LIVE_BUILTINS.iter().any(|(id, _)| *id == preset) || custom.iter().any(|view| view.id == preset)
}

fn reading_known(preset: &str, custom: &[NamedView]) -> bool {
    READING_BUILTINS.iter().any(|(id, _)| *id == preset) || custom.iter().any(|view| view.id == preset)
}

fn live_choices(custom: &[NamedView]) -> Vec<(String, String)> {
    let mut choices: Vec<(String, String)> = LIVE_BUILTINS
        .iter()
        .map(|(id, name)| ((*id).to_string(), (*name).to_string()))
        .collect();
    choices.extend(custom.iter().map(|view| (view.id.clone(), view.name.clone())));
    choices
}

fn live_label(preset: &str, custom: &[NamedView]) -> String {
    live_choices(custom)
        .into_iter()
        .find(|(id, _)| id == preset)
        .map(|(_, name)| name)
        .unwrap_or_else(|| "Pack".into())
}

fn live_used(custom: &[NamedView]) -> Vec<String> {
    let mut used: Vec<String> = LIVE_BUILTINS.iter().map(|(_, name)| (*name).to_string()).collect();
    used.extend(custom.iter().map(|view| view.name.clone()));
    used
}

fn builtin_live(preset: &str) -> &'static [&'static str] {
    match preset {
        "ride" => &["speed", "range", "soc", "packVoltage"],
        "charge" => &["packVoltage", "current", "soc"],
        "drive" => &["speed", "rpm", "motor", "inverter"],
        _ => &["packVoltage", "current", "tempA"],
    }
}

fn live_indexes(preset: &str, custom: &[NamedView]) -> (bool, Vec<usize>) {
    if preset == "all" {
        return (true, (0..SERIES.len()).collect());
    }
    let stored = custom.iter().find(|view| view.id == preset).map(|view| view.keys.clone());
    let indexes = SERIES
        .iter()
        .enumerate()
        .filter(|(_, series)| match &stored {
            Some(keys) => keys.iter().any(|key| key == series.key),
            None => builtin_live(preset).contains(&series.key),
        })
        .map(|(index, _)| index)
        .collect();
    (false, indexes)
}

fn chart_indexes(preset: &str, custom: &[NamedView], on: &[bool]) -> Vec<usize> {
    let (composing, tiles) = live_indexes(preset, custom);
    if composing {
        tiles.into_iter().filter(|index| on[*index]).collect()
    } else {
        tiles
    }
}

fn reading_choices(custom: &[NamedView]) -> Vec<(String, String)> {
    let mut choices: Vec<(String, String)> = READING_BUILTINS
        .iter()
        .map(|(id, name)| ((*id).to_string(), (*name).to_string()))
        .collect();
    choices.extend(custom.iter().map(|view| (view.id.clone(), view.name.clone())));
    choices
}

fn reading_label(preset: &str, custom: &[NamedView]) -> String {
    reading_choices(custom)
        .into_iter()
        .find(|(id, _)| id == preset)
        .map(|(_, name)| name)
        .unwrap_or_else(|| "All".into())
}

fn reading_used(custom: &[NamedView]) -> Vec<String> {
    let mut used: Vec<String> = READING_BUILTINS.iter().map(|(_, name)| (*name).to_string()).collect();
    used.extend(custom.iter().map(|view| view.name.clone()));
    used
}

fn reading_keys(preset: &str, custom: &[NamedView]) -> (bool, Vec<&'static str>) {
    if preset == "all" {
        return (true, readings::keys());
    }
    if let Some(view) = custom.iter().find(|view| view.id == preset) {
        let keys = readings::rows()
            .iter()
            .filter(|row| view.keys.iter().any(|key| key == row.key))
            .map(|row| row.key)
            .collect();
        return (false, keys);
    }
    let group = match preset {
        "cells" => "Cells",
        "scooter" => "Scooter",
        "motor" => "Motor",
        _ => "Pack",
    };
    (false, readings::group_keys(group))
}

fn sample_field(sample: &Sample, key: &str) -> Option<f32> {
    sample.field(key)
}

fn format_series(series: &Series, value: f32) -> String {
    if series.whole {
        format!("{}", value.round() as i32)
    } else {
        format!("{value:.1}")
    }
}

fn series_detail(series: &Series, value: Option<f32>) -> Detail {
    Detail {
        kicker: "Live".into(),
        title: series.title.into(),
        value: value
            .map(|number| format!("{} {}", format_series(series, number), series.unit))
            .unwrap_or_default(),
        location: series.location.into(),
        text: series.text.into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_chart_image(
    ctx: &egui::Context,
    width: f32,
    split: bool,
    window_min: u32,
    rows: &[Sample],
    indexes: &[usize],
    chart_note: &str,
    marks: &[Mark],
) -> (u32, u32, Vec<u8>) {
    if width < 1.0 {
        return (0, 0, Vec::new());
    }
    let layer_id = egui::LayerId::new(egui::Order::Background, egui::Id::unique("chart-export"));
    let start = ctx.graphics_mut(|graphics| graphics.entry(layer_id).next_idx().0);
    let origin = egui::pos2(0.0, 100_000.0);
    let max_rect = egui::Rect::from_min_size(origin, egui::vec2(width, 20_000.0));
    let mut ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::unique("chart-export-ui"),
        egui::UiBuilder::new()
            .layer_id(layer_id)
            .max_rect(max_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    let card_rect = ui
        .scope(|ui| {
            card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                    ui.label(RichText::new("Stream").size(16.0).color(INK));
                    ui.label(RichText::new(format!("{window_min} min")).size(15.0).color(INK));
                    ui.label(RichText::new(if split { "Split" } else { "Overlay" }).size(15.0).color(INK));
                });
                ui.add_space(8.0);
                let _hover = chart_graphs(ui, split, rows, indexes, chart_note, marks, CHART_HEIGHTS);
            });
        })
        .response
        .rect;
    drop(ui);
    let mut shapes = claim_shapes(ctx, layer_id, start);
    let shift = egui::emath::TSTransform::from_translation(egui::vec2(-card_rect.min.x, -card_rect.min.y));
    for shape in &mut shapes {
        shape.transform(shift);
    }
    let pixels_per_point = ctx.pixels_per_point().max(0.1);
    let pixel_w = (card_rect.width() * pixels_per_point).round() as u32;
    let pixel_h = (card_rect.height() * pixels_per_point).round() as u32;
    if pixel_w == 0 || pixel_h == 0 {
        return (0, 0, Vec::new());
    }
    let primitives = ctx.tessellate(shapes, pixels_per_point);
    let atlas = ctx.fonts(|fonts| fonts.image());
    let rgba = chart_image::rasterize(pixel_w, pixel_h, pixels_per_point, &primitives, &atlas, PANEL);
    (pixel_w, pixel_h, rgba)
}

fn claim_shapes(ctx: &egui::Context, layer_id: egui::LayerId, start: usize) -> Vec<egui::epaint::ClippedShape> {
    ctx.graphics_mut(|graphics| {
        let list = graphics.entry(layer_id);
        let end = list.next_idx().0;
        let shapes = list.all_entries().skip(start).cloned().collect();
        for index in start..end {
            list.reset_shape(egui::layers::ShapeIdx(index));
        }
        shapes
    })
}

fn chart_graphs(
    ui: &mut egui::Ui,
    split: bool,
    rows: &[Sample],
    indexes: &[usize],
    chart_note: &str,
    marks: &[Mark],
    heights: (f32, f32),
) -> String {
    let mut hover = String::new();
    if indexes.is_empty() {
        ui.label(RichText::new("Turn a tile on to draw it.").color(MUTED));
    } else if split {
        for (step, (unit, members)) in unit_groups(indexes).into_iter().enumerate() {
            if step > 0 {
                ui.add_space(8.0);
            }
            if let Some(text) = paint_split(ui, rows, unit, &members, marks, heights.1) {
                hover = text;
            }
        }
        ui.add_space(8.0);
        chart_key(ui, indexes);
    } else {
        if let Some(text) = paint_overlay(ui, rows, indexes, marks, heights.0) {
            hover = text;
        }
        ui.add_space(8.0);
        chart_key(ui, indexes);
    }
    if !hover.is_empty() {
        ui.label(RichText::new(&hover).color(INK));
    }
    let note = if split {
        "Readings that share a unit share an axis."
    } else {
        "Each line is stretched to its own range, so the height is not one shared unit."
    };
    ui.label(RichText::new(note).size(12.0).color(MUTED));
    if marks_in_view(marks, rows) {
        ui.label(
            RichText::new("Numbered amber lines are your event markers. They are listed on the Frames screen.")
                .size(12.0)
                .color(MUTED),
        );
    }
    if !chart_note.is_empty() {
        ui.label(RichText::new(chart_note).size(12.0).color(GOOD));
    }
    hover
}

fn chart_name(key: &str) -> &str {
    match key {
        "packVoltage" => "Voltage",
        "range" => "Range",
        "rpm" => "Motor",
        _ => SERIES.iter().find(|series| series.key == key).map(|series| series.title).unwrap_or(key),
    }
}

fn chart_key(ui: &mut egui::Ui, indexes: &[usize]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(14.0, 8.0);
        for index in indexes {
            let series = &SERIES[*index];
            let text = format!("{} {}", chart_name(series.key), series.unit);
            let galley = ui.painter().layout_no_wrap(text, egui::FontId::proportional(12.0), INK);
            let width = 14.0 + 6.0 + galley.size().x;
            let height = galley.size().y.max(12.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
            let bar = egui::Rect::from_center_size(
                egui::pos2(rect.left() + 7.0, rect.center().y),
                Vec2::new(14.0, 3.0),
            );
            ui.painter().rect_filled(bar, CornerRadius::same(2), series.color);
            ui.painter().galley(
                egui::pos2(rect.left() + 20.0, rect.center().y - galley.size().y / 2.0),
                galley,
                INK,
            );
        }
    });
}

fn paint_overlay(ui: &mut egui::Ui, rows: &[Sample], indexes: &[usize], marks: &[Mark], height: f32) -> Option<String> {
    let (outer, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let gutter = 86.0_f32.min((outer.width() - 16.0).max(0.0));
    let plot = egui::Rect::from_min_max(
        egui::pos2(outer.left() + 16.0_f32.min(outer.width()), outer.top() + 16.0),
        egui::pos2(outer.right() - gutter, (outer.bottom() - 28.0).max(outer.top() + 16.0)),
    );
    let painter = ui.painter().with_clip_rect(outer);
    painter.rect_filled(outer, CornerRadius::same(10), CHART_BG);
    paint_marks(&painter, marks, rows, plot);
    let mut labels = Vec::new();
    for index in indexes {
        let series = &SERIES[*index];
        let values = series_values(rows, series.key);
        if values.is_empty() {
            continue;
        }
        let (min, max) = value_span(&values);
        let points = screen_points(rows, &values, plot, min, max, false);
        paint_trace(&painter, &points, series.color);
        if let Some((_, latest)) = values.last() {
            labels.push(EndLabel {
                y: y_at(f64::from(*latest), min, max, plot, false),
                text: format!("{} {}", format_series(series, *latest), series.unit),
                color: series.color,
            });
        }
    }
    let font = egui::FontId::proportional(12.0);
    for label in place_labels(labels, plot.top(), plot.bottom()) {
        painter.text(
            egui::pos2(plot.right() + 8.0, label.y),
            egui::Align2::LEFT_CENTER,
            label.text,
            font.clone(),
            label.color,
        );
    }
    paint_time_axis(&painter, rows, plot, outer.bottom() - 10.0);
    hover_at(rows, indexes, response.hover_pos(), plot, true, 0.0, 1.0)
}

fn paint_split(
    ui: &mut egui::Ui,
    rows: &[Sample],
    unit: &str,
    members: &[usize],
    marks: &[Mark],
    height: f32,
) -> Option<String> {
    let (outer, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let inset = 58.0_f32.min((outer.width() / 2.0).max(0.0));
    let plot = egui::Rect::from_min_max(
        egui::pos2(outer.left() + inset, outer.top() + 28.0),
        egui::pos2(outer.right() - inset, (outer.bottom() - 28.0).max(outer.top() + 28.0)),
    );
    let painter = ui.painter().with_clip_rect(outer);
    painter.rect_filled(outer, CornerRadius::same(10), CHART_BG);
    let font = egui::FontId::proportional(12.0);
    painter.text(egui::pos2(outer.left() + 8.0, outer.top() + 14.0), egui::Align2::LEFT_CENTER, unit, font.clone(), INK);
    painter.text(egui::pos2(outer.right() - 8.0, outer.top() + 14.0), egui::Align2::RIGHT_CENTER, unit, font.clone(), INK);
    let values = members
        .iter()
        .flat_map(|index| series_values(rows, SERIES[*index].key).into_iter().map(|(_, value)| f64::from(value)))
        .collect::<Vec<_>>();
    let (min, max) = if values.is_empty() {
        (0.0, 1.0)
    } else {
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (min, max)
    };
    for (y, value) in [
        (plot.top(), max),
        ((plot.top() + plot.bottom()) / 2.0, (min + max) / 2.0),
        (plot.bottom(), min),
    ] {
        let label = format_tick(unit, value);
        painter.text(egui::pos2(plot.left() - 8.0, y), egui::Align2::RIGHT_CENTER, &label, font.clone(), MUTED);
        painter.text(egui::pos2(plot.right() + 8.0, y), egui::Align2::LEFT_CENTER, &label, font.clone(), MUTED);
        painter.line_segment([egui::pos2(plot.left() - 4.0, y), egui::pos2(plot.left(), y)], Stroke::new(1.0, LINE_COLOR));
        painter.line_segment([egui::pos2(plot.right(), y), egui::pos2(plot.right() + 4.0, y)], Stroke::new(1.0, LINE_COLOR));
    }
    paint_marks(&painter, marks, rows, plot);
    for index in members {
        let series = &SERIES[*index];
        let series_rows = series_values(rows, series.key);
        let points = screen_points(rows, &series_rows, plot, min, max, true);
        paint_trace(&painter, &points, series.color);
    }
    paint_time_axis(&painter, rows, plot, outer.bottom() - 10.0);
    hover_at(rows, members, response.hover_pos(), plot, false, min, max)
}

/// True when at least one marker falls inside the drawn time span.
fn marks_in_view(marks: &[Mark], rows: &[Sample]) -> bool {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return false;
    };
    rows.len() >= 2 && marks.iter().any(|mark| mark.t_ms >= first.t_ms && mark.t_ms <= last.t_ms)
}

/// A thin amber line for each marker inside the window, numbered as in the
/// marker list on the Frames screen.
fn paint_marks(painter: &egui::Painter, marks: &[Mark], rows: &[Sample], plot: egui::Rect) {
    if !marks_in_view(marks, rows) {
        return;
    }
    let font = egui::FontId::proportional(12.0);
    for (index, mark) in marks.iter().enumerate() {
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
            return;
        };
        if mark.t_ms < first.t_ms || mark.t_ms > last.t_ms {
            continue;
        }
        let x = x_at(mark.t_ms, rows, plot);
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            Stroke::new(1.0, WARN.gamma_multiply(0.75)),
        );
        painter.text(
            egui::pos2(x + 3.0, plot.top() - 2.0),
            egui::Align2::LEFT_TOP,
            format!("{}", index + 1),
            font.clone(),
            WARN,
        );
    }
}

fn unit_groups(indexes: &[usize]) -> Vec<(&'static str, Vec<usize>)> {
    UNIT_ORDER
        .into_iter()
        .filter_map(|unit| {
            let members: Vec<usize> = indexes
                .iter()
                .copied()
                .filter(|index| SERIES[*index].unit == unit)
                .collect();
            if members.is_empty() { None } else { Some((unit, members)) }
        })
        .collect()
}

fn format_tick(unit: &str, value: f64) -> String {
    if matches!(unit, "%" | "km/h" | "km" | "rpm") {
        format!("{}", value.round() as i32)
    } else {
        format!("{value:.1}")
    }
}

struct EndLabel {
    y: f32,
    text: String,
    color: Color32,
}

fn series_values(rows: &[Sample], key: &str) -> Vec<(u64, f32)> {
    rows.iter()
        .filter_map(|row| sample_field(row, key).map(|value| (row.t_ms, value)))
        .collect()
}

fn value_span(values: &[(u64, f32)]) -> (f64, f64) {
    let min = values.iter().map(|(_, value)| f64::from(*value)).fold(f64::INFINITY, f64::min);
    let max = values.iter().map(|(_, value)| f64::from(*value)).fold(f64::NEG_INFINITY, f64::max);
    (min, max)
}

fn y_at(value: f64, min: f64, max: f64, plot: egui::Rect, flat_middle: bool) -> f32 {
    if (max - min).abs() < 1e-9 {
        return if flat_middle { (plot.top() + plot.bottom()) / 2.0 } else { plot.bottom() };
    }
    let fraction = ((value - min) / (max - min)) as f32;
    plot.bottom() - fraction * plot.height()
}

fn x_at(t: u64, rows: &[Sample], plot: egui::Rect) -> f32 {
    if rows.len() < 2 {
        return plot.right();
    }
    let start = rows[0].t_ms;
    let span = rows.last().map(|row| row.t_ms).unwrap_or(start).saturating_sub(start);
    if span == 0 {
        return plot.right();
    }
    plot.left() + (t.saturating_sub(start) as f32 / span as f32) * plot.width()
}

fn screen_points(
    rows: &[Sample],
    values: &[(u64, f32)],
    plot: egui::Rect,
    min: f64,
    max: f64,
    flat_middle: bool,
) -> Vec<egui::Pos2> {
    values
        .iter()
        .map(|(t, value)| egui::pos2(x_at(*t, rows, plot), y_at(f64::from(*value), min, max, plot, flat_middle)))
        .collect()
}

fn paint_trace(painter: &egui::Painter, points: &[egui::Pos2], color: Color32) {
    if points.len() >= 2 {
        painter.line(points.to_vec(), Stroke::new(2.0, color));
    } else if let Some(point) = points.first() {
        painter.circle_filled(*point, 3.0, color);
    }
}

fn place_labels(mut labels: Vec<EndLabel>, top: f32, bottom: f32) -> Vec<EndLabel> {
    labels.sort_by(|left, right| left.y.partial_cmp(&right.y).unwrap_or(std::cmp::Ordering::Equal));
    for index in 1..labels.len() {
        let floor = labels[index - 1].y + 16.0;
        if labels[index].y < floor {
            labels[index].y = floor;
        }
    }
    if labels.is_empty() {
        return labels;
    }
    let overflow = labels.last().unwrap().y - bottom;
    if overflow > 0.0 {
        for label in &mut labels {
            label.y -= overflow;
        }
    }
    let underflow = top - labels[0].y;
    if underflow > 0.0 {
        for label in &mut labels {
            label.y += underflow;
        }
    }
    labels
}

fn paint_time_axis(painter: &egui::Painter, rows: &[Sample], plot: egui::Rect, y: f32) {
    let font = egui::FontId::proportional(12.0);
    let newest = rows.last().map(|row| row.t_ms).unwrap_or(0);
    let oldest = rows.first().map(|row| row.t_ms).unwrap_or(newest);
    let span = newest.saturating_sub(oldest);
    let left = if span > 0 { snapshot::format_age(newest, oldest) } else { "0 s".into() };
    let middle = if span > 0 { snapshot::format_age(newest, oldest + span / 2) } else { "0 s".into() };
    painter.text(egui::pos2(plot.left(), y), egui::Align2::LEFT_CENTER, left, font.clone(), MUTED);
    painter.text(egui::pos2(plot.center().x, y), egui::Align2::CENTER_CENTER, middle, font.clone(), MUTED);
    painter.text(egui::pos2(plot.right(), y), egui::Align2::RIGHT_CENTER, "now", font, MUTED);
}

fn hover_at(
    rows: &[Sample],
    indexes: &[usize],
    pointer: Option<egui::Pos2>,
    plot: egui::Rect,
    overlay: bool,
    min: f64,
    max: f64,
) -> Option<String> {
    let pointer = pointer.filter(|pos| plot.contains(*pos))?;
    let newest = rows.last()?.t_ms;
    let span = newest.saturating_sub(rows.first().map(|row| row.t_ms).unwrap_or(newest));
    let t = if span == 0 {
        newest
    } else {
        let fraction = ((pointer.x - plot.left()) / plot.width()).clamp(0.0, 1.0);
        rows[0].t_ms + (fraction * span as f32) as u64
    };
    let pointer_y = if overlay {
        let fraction = ((plot.bottom() - pointer.y) / plot.height()).clamp(0.0, 1.0);
        f64::from(fraction)
    } else {
        let fraction = ((plot.bottom() - pointer.y) / plot.height()).clamp(0.0, 1.0);
        min + f64::from(fraction) * (max - min)
    };
    let mut best: Option<(f64, String)> = None;
    for index in indexes {
        let series = &SERIES[*index];
        let values = series_values(rows, series.key);
        if values.is_empty() {
            continue;
        }
        let (series_min, series_max) = if overlay { value_span(&values) } else { (min, max) };
        let Some(nearest) = values.iter().min_by_key(|(stamp, _)| stamp.abs_diff(t)) else {
            continue;
        };
        let y = if overlay {
            if (series_max - series_min).abs() < 1e-9 {
                0.0
            } else {
                (f64::from(nearest.1) - series_min) / (series_max - series_min)
            }
        } else {
            f64::from(nearest.1)
        };
        let distance = (y - pointer_y).abs();
        let text = format!(
            "{} {} {} · {}",
            series.title,
            format_series(series, nearest.1),
            series.unit,
            snapshot::format_age(newest, nearest.0)
        );
        if best.as_ref().is_none_or(|current| distance < current.0) {
            best = Some((distance, text));
        }
    }
    best.map(|(_, text)| text)
}

/// The 14 cell bars. Compact puts them in two rows of seven, so each bar is
/// wide enough on a phone to carry its number.
fn cells(ui: &mut egui::Ui, snap: &Snapshot, detail: &mut Option<Detail>, compact: bool) {
    let spread = CellSpread::of(&snap.cells);
    let median = spread.map(|spread| spread.median);
    let summary = spread.map_or_else(|| "— mV (cell —)   — mV (cell —)   median — mV".to_string(), |spread| spread.summary());
    let hover = spread.map_or_else(
        || "The cells have not been heard yet.".to_string(),
        |spread| {
            format!(
                "Highest minus lowest: {} mV. The median is the middle cell voltage, so one cell far off does not move it.",
                mv_text(spread.delta())
            )
        },
    );
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Cells").size(16.0).color(INK));
            if !compact {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(&summary).color(MUTED)).on_hover_text(&hover);
                });
            }
        });
        if compact {
            ui.add(egui::Label::new(RichText::new(&summary).size(13.0).color(MUTED)).wrap());
        }
        ui.add_space(10.0);
        let width = ui.available_width();
        let gap = 6.0;
        let per_row: usize = if compact { 7 } else { 14 };
        let bar_height = if compact { 110.0 } else { 200.0 };
        let across = per_row as f32;
        let fitted = (width - gap * (across - 1.0)) / across;
        let column = if across * 24.0 + gap * (across - 1.0) <= width { fitted.max(24.0) } else { fitted.max(0.0) };
        for first in (0..14).step_by(per_row) {
            if first > 0 {
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for index in first..first + per_row {
                    let millivolts = snap.cells[index];
                    let marked = match (millivolts, median) {
                        (Some(value), Some(median)) => (value - median).abs() > 30.0,
                        _ => false,
                    };
                    let (rect, response) = ui.allocate_exact_size(Vec2::new(column, bar_height), Sense::click());
                    paint_cell(ui, rect, index, millivolts, marked, median);
                    if response.clicked() {
                        *detail = Some(Detail {
                            kicker: "Cell".into(),
                            title: format!("Cell {}", index + 1),
                            value: millivolts.map(|value| format!("{} mV", value.round() as i32)).unwrap_or_default(),
                            location: format!("{}, little-endian millivolts", explain::cell_frame(index)),
                            text: cell_text(spread, millivolts),
                        });
                    }
                }
            });
        }
    });
}

/// What the detail panel says about one cell: its distance from the median,
/// and the spread across the pack (the figure the card no longer prints).
fn cell_text(spread: Option<CellSpread>, millivolts: Option<f32>) -> String {
    let mut text = "Cell voltage in millivolts.".to_string();
    if let (Some(spread), Some(value)) = (spread, millivolts) {
        text.push_str(&format!(
            " This cell is {} mV from the median of the cells ({} mV). Highest minus lowest across the pack: {} mV.",
            signed_mv(spread.offset(value)),
            mv_text(spread.median),
            mv_text(spread.delta())
        ));
    }
    text
}

fn paint_cell(ui: &mut egui::Ui, rect: egui::Rect, index: usize, millivolts: Option<f32>, marked: bool, median: Option<f32>) {
    let label_top = rect.bottom() - 36.0;
    let area = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), label_top));
    let fraction = match (millivolts, median) {
        (Some(value), Some(median)) => {
            let lo = median - 80.0;
            let hi = median + 80.0;
            ((value - lo) / (hi - lo)).clamp(0.08, 1.0)
        }
        (Some(_), None) => 0.7,
        _ => 0.08,
    };
    let bar_height = area.height() * fraction;
    let bar = egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - bar_height), area.right_bottom());
    let color = if marked { WARN } else { GOOD };
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(bar, CornerRadius::same(4), color);
    let center = rect.center().x;
    painter.text(egui::pos2(center, label_top + 8.0), egui::Align2::CENTER_CENTER, format!("{}", index + 1), egui::FontId::proportional(11.0), MUTED);
    let reading = millivolts.map(|value| format!("{}", value.round() as i32)).unwrap_or_else(|| "—".into());
    painter.text(egui::pos2(center, label_top + 24.0), egui::Align2::CENTER_CENTER, format!("{reading} mV"), egui::FontId::proportional(10.0), MUTED);
}

fn set_flags(ui: &mut egui::Ui, snap: &Snapshot, detail: &mut Option<Detail>) -> bool {
    let set = explain::set_flags(snap);
    let mut open_flags = false;
    card(ui, |ui| {
        // The flags wrap, so the card would shrink to fit them. Take the whole
        // width instead, so its right edge lines up with the other cards.
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.label(RichText::new("Set flags").size(16.0).color(INK));
            if ui.button("All flags").clicked() {
                open_flags = true;
            }
        });
        ui.add_space(8.0);
        if set.is_empty() {
            ui.label(RichText::new("No flags set.").color(MUTED));
            return;
        }
        flag_wrap(ui, snap, &set, detail, true);
    });
    open_flags
}

fn flags_page(ui: &mut egui::Ui, snap: &Snapshot, detail: &mut Option<Detail>) {
    let mut groups: Vec<(&str, Vec<&explain::FlagDef>)> = Vec::new();
    for flag in explain::catalogue() {
        if groups.last().is_some_and(|(name, _)| *name == flag.group) {
            groups.last_mut().unwrap().1.push(flag);
        } else {
            groups.push((flag.group, vec![flag]));
        }
    }
    for (name, flags) in groups {
        ui.add_space(10.0);
        ui.label(RichText::new(name.to_uppercase()).size(13.0).color(MUTED));
        ui.add_space(8.0);
        if flags.iter().all(|flag| snap.bytes_for(flag.id).is_none()) {
            let note = if name.starts_with("ECU") {
                "No frames yet. The ECU's inputs, outputs and status have only been seen with debug mode on (the switch is above)."
            } else {
                "No frames yet."
            };
            ui.label(RichText::new(note).size(12.0).color(MUTED));
            ui.add_space(6.0);
        }
        flag_wrap(ui, snap, &flags, detail, false);
        ui.add_space(8.0);
    }
}

fn flag_wrap(ui: &mut egui::Ui, snap: &Snapshot, flags: &[&explain::FlagDef], detail: &mut Option<Detail>, hide_clear: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(10.0, 5.0);
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
        let row_width = ui.available_width();
        for flag in flags {
            let set = explain::is_set(snap, flag);
            if hide_clear && !set {
                continue;
            }
            let (text, border) = if !set {
                (MUTED, LINE_COLOR)
            } else {
                match explain::tone(flag.name) {
                    Tone::Alarm => (ALARM, Color32::from_rgb(0x6d, 0x3a, 0x34)),
                    Tone::Warn => (WARN, Color32::from_rgb(0x6d, 0x5a, 0x32)),
                    Tone::Lit => (GOOD, Color32::from_rgb(0x3d, 0x5a, 0x40)),
                }
            };
            let wanted = text_width(ui, flag.name, 15.0) + 20.0;
            let left = ui.available_size_before_wrap().x;
            if wanted > left && left + 1.0 < row_width {
                ui.end_row();
            }
            let button = Button::new(RichText::new(flag.name).color(text))
                .fill(PANEL2)
                .stroke(Stroke::new(1.0, border))
                .corner_radius(CornerRadius::same(100))
                .truncate();
            let response = if wanted <= row_width {
                ui.add(button)
            } else {
                ui.add_sized([row_width, 29.0], button)
            };
            if response.clicked() {
                *detail = Some(Detail {
                    kicker: flag.group.into(),
                    title: flag.name.into(),
                    value: if set { "Set".into() } else { "Clear".into() },
                    location: explain::where_text(flag),
                    text: explain::flag_detail(flag),
                });
            }
        }
    });
}

fn param_row(ui: &mut egui::Ui, name: &str, value: &str, unit: &str, unit_width: f32) -> egui::Response {
    param_row_colored(ui, name, value, unit, unit_width, INK)
}

fn param_row_colored(
    ui: &mut egui::Ui,
    name: &str,
    value: &str,
    unit: &str,
    unit_width: f32,
    value_color: Color32,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 29.0), Sense::click());
    let border = if response.hovered() { HOVER_BORDER } else { LINE_COLOR };
    let painter = ui.painter();
    painter.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, border));
    let font = egui::FontId::proportional(15.0);
    let y = rect.center().y;
    painter.text(egui::pos2(rect.right(), y), egui::Align2::RIGHT_CENTER, unit, font.clone(), MUTED);
    painter.text(
        egui::pos2(rect.right() - unit_width - 10.0, y),
        egui::Align2::RIGHT_CENTER,
        value,
        font.clone(),
        value_color,
    );
    painter.text(egui::pos2(rect.left(), y), egui::Align2::LEFT_CENTER, name, font, INK);
    response
}

fn param_sections(
    ui: &mut egui::Ui,
    sections: &[Section],
    values: Option<&[String]>,
    changed: &[bool],
    detail: &mut Option<Detail>,
) {
    let mut index = 0;
    for section in sections {
        ui.add_space(8.0);
        ui.label(RichText::new(section.title.to_uppercase()).size(13.0).color(MUTED));
        for row in section.rows {
            let name = row.name;
            let unit = row.unit;
            let value = values.and_then(|list| list.get(index)).map(String::as_str).unwrap_or("—");
            let color = if changed.get(index).copied().unwrap_or(false) { WARN } else { INK };
            if param_row_colored(ui, name, value, unit, 64.0, color).clicked() {
                let shown = if value == "—" {
                    String::new()
                } else if unit.is_empty() {
                    value.to_string()
                } else {
                    format!("{value} {unit}")
                };
                *detail = Some(Detail {
                    kicker: "Parameter".into(),
                    title: name.into(),
                    value: shown,
                    location: if unit.is_empty() {
                        "Catalogue entry".into()
                    } else {
                        format!("Catalogue units: {unit}")
                    },
                    text: if unit.is_empty() {
                        name.into()
                    } else {
                        format!("{name}, {unit}.")
                    },
                });
            }
            index += 1;
        }
    }
}

fn sparkline(ui: &mut egui::Ui, values: &[f64], color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    if values.len() < 2 {
        return;
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(1e-9);
    let plot = rect.shrink2(Vec2::new(0.0, 4.0));
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let x = plot.left() + plot.width() * index as f32 / (values.len() - 1) as f32;
            let y = plot.bottom() - plot.height() * ((*value - min) / span) as f32;
            egui::pos2(x, y)
        })
        .collect();
    ui.painter().line(points, Stroke::new(1.6, color));
}

fn card(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    panel_frame(PANEL, 12)
        .stroke(Stroke::new(1.0, LINE_COLOR))
        .corner_radius(CornerRadius::same(14))
        .show(ui, body);
}

fn panel_frame(fill: Color32, margin: i8) -> Frame {
    Frame::NONE
        .fill(fill)
        .inner_margin(Margin::same(margin))
        .corner_radius(CornerRadius::same(0))
}

fn frame_widget(widget: &mut egui::style::WidgetVisuals, border: Color32) {
    widget.weak_bg_fill = PANEL2;
    widget.bg_fill = PANEL2;
    widget.bg_stroke = Stroke::new(1.0, border);
    widget.fg_stroke = Stroke::new(1.0, INK);
    widget.corner_radius = CornerRadius::same(8);
}

/// The header shows only the last three digits. The full number is on hover.
fn pack_label(id: Option<&str>) -> String {
    match id {
        None => "Pack —".into(),
        Some(id) => {
            let skip = id.chars().count().saturating_sub(3);
            let tail: String = id.chars().skip(skip).collect();
            if skip == 0 {
                format!("Pack {tail}")
            } else {
                format!("Pack …{tail}")
            }
        }
    }
}

fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG;
    visuals.panel_fill = BG;
    visuals.extreme_bg_color = CHART_BG;
    visuals.faint_bg_color = PANEL;
    visuals.code_bg_color = PANEL2;
    visuals.text_edit_bg_color = Some(PANEL2);
    visuals.widgets.noninteractive.weak_bg_fill = PANEL2;
    visuals.widgets.noninteractive.bg_fill = PANEL2;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE_COLOR);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);
    frame_widget(&mut visuals.widgets.inactive, LINE_COLOR);
    frame_widget(&mut visuals.widgets.hovered, HOVER_BORDER);
    frame_widget(&mut visuals.widgets.active, HOVER_BORDER);
    frame_widget(&mut visuals.widgets.open, LINE_COLOR);
    visuals.selection.bg_fill = PANEL2;
    visuals.selection.stroke = Stroke::new(1.0, HOVER_BORDER);
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            style.visuals = visuals.clone();
            style.spacing.button_padding = Vec2::new(12.0, 7.0);
            style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version named by the newest "## x.y.z (date)" heading of a changelog.
    fn newest_changelog_version(log: &str) -> Option<&str> {
        let heading = log.lines().find(|line| line.starts_with("## "))?;
        heading.strip_prefix("## ")?.split_whitespace().next()
    }

    #[test]
    fn the_changelog_leads_with_an_entry_for_this_version() {
        let log = include_str!("../../CHANGELOG.md");
        assert_eq!(
            newest_changelog_version(log),
            Some(VERSION),
            "CHANGELOG.md must start with an entry for version {VERSION}. Every change that \
             alters behaviour raises the version in Cargo.toml and adds its entry in the same commit."
        );
    }

    #[test]
    fn the_changelog_check_notices_a_missing_or_stale_entry() {
        let newer = "# Changelog\n\n## 0.3.0 (1 Jan 2027)\n\n## 0.2.1 (5 Nov 2026)\n";
        assert_eq!(newest_changelog_version(newer), Some("0.3.0"));
        assert_eq!(newest_changelog_version("# Changelog\n\n## Unreleased\n"), Some("Unreleased"));
        assert_eq!(newest_changelog_version("# Changelog\n"), None);
    }

    fn app_for_tests() -> MonitorApp {
        let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
        MonitorApp::new(&cc)
    }

    /// Waits until a thread has ended, for up to two seconds.
    fn wait_for_end(app: &MonitorApp) {
        for _ in 0..200 {
            if app.worker.as_ref().is_some_and(|worker| worker.is_finished()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_feed_thread_that_crashes_is_noticed_and_reported() {
        let mut app = app_for_tests();
        app.link = Link::Can;
        app.notice = "Connected · test".into();
        let (release, hold) = mpsc::channel::<()>();
        app.worker = Some(std::thread::spawn(move || {
            let _ = hold.recv();
            panic!("test feed crashed");
        }));
        // While it runs, nothing changes.
        app.check_worker();
        assert!(app.link == Link::Can && !app.notice_bad && app.worker.is_some());
        release.send(()).unwrap();
        wait_for_end(&app);
        app.check_worker();
        assert!(app.link == Link::Idle, "the app should have disconnected");
        assert!(app.notice_bad);
        assert!(app.notice.contains("test feed crashed"), "{}", app.notice);
        assert!(app.worker.is_none());
    }

    #[test]
    fn a_feed_that_ended_with_an_error_still_shows_that_error() {
        let mut app = app_for_tests();
        app.link = Link::Can;
        lock(&app.snapshot).error = Some("Could not open COM3: busy".into());
        app.worker = Some(std::thread::spawn(|| {}));
        wait_for_end(&app);
        app.check_worker();
        assert!(app.link == Link::Idle);
        assert_eq!(app.notice, "Could not open COM3: busy");
        assert!(app.notice_bad);
    }

    #[test]
    fn a_feed_that_is_still_running_is_left_alone() {
        let mut app = app_for_tests();
        app.link = Link::Sim;
        let (release, hold) = mpsc::channel::<()>();
        app.worker = Some(std::thread::spawn(move || {
            let _ = hold.recv();
        }));
        for _ in 0..5 {
            app.check_worker();
        }
        assert!(app.link == Link::Sim && !app.notice_bad && app.worker.is_some());
        release.send(()).unwrap();
        wait_for_end(&app);
        app.worker.take().map(|worker| worker.join());
    }

    /// Draws the whole app once on a window of this size.
    fn draw_once(app: &mut MonitorApp, ctx: &egui::Context, width: f32, height: f32) {
        let mut frame = eframe::Frame::_new_kittest();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height)));
        app.sync_signals();
        let output = ctx.run_ui(input, |ui| {
            apply_theme(ui.ctx());
            eframe::App::ui(app, ui, &mut frame);
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn the_explore_screen_draws_at_every_size_with_and_without_data() {
        use crate::snapshot::Frame as CanFrame;
        let known = SignalDef {
            key: "signal-1".into(),
            name: "Horn".into(),
            id: 0x182,
            start: 2,
            count: 2,
            signed: true,
            big_endian: false,
            scale: 0.1,
            offset: -40.0,
            unit: "°C".into(),
        };
        for (width, height) in [(360.0, 700.0), (390.0, 1500.0), (640.0, 800.0), (900.0, 700.0), (1180.0, 860.0)] {
            for with_data in [false, true] {
                let ctx = egui::Context::default();
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut app = MonitorApp::new(&cc);
                app.screen = Screen::Explore;
                app.signal_defs = vec![known.clone()];
                if with_data {
                    app.link = Link::Replay;
                    app.sync_signals();
                    let mut snapshot = lock(&app.snapshot);
                    for step in 0..60u64 {
                        let t = 1_000 + step * 100;
                        snapshot.tap(&CanFrame::new(0x182, &[0, 1, (step % 7) as u8, (step / 30) as u8]), t);
                        snapshot.tap(&CanFrame::new(0x280, &[0x21, 0x08, 0, 0]), t);
                        if step == 20 {
                            snapshot.mark_at("horn off", t);
                        }
                        if step == 50 {
                            snapshot.mark_at("", t);
                        }
                    }
                }
                for tab in [explore::Tab::Signals, explore::Tab::Compare] {
                    app.explore.tab = tab;
                    app.explore.draft.id = "182".into();
                    app.explore.draft.count = 2;
                    for _ in 0..3 {
                        draw_once(&mut app, &ctx, width, height);
                    }
                    // A form with something wrong in it, a delete question and
                    // the other end of the pickers all draw too.
                    app.explore.draft.id = "zz".into();
                    app.explore.draft.scale = "x".into();
                    app.explore.confirm_delete = Some(("signal-1".into(), "Horn".into()));
                    app.explore.compare_a = Some(1);
                    app.explore.compare_b = Some(0);
                    app.explore.hide_noisy = false;
                    draw_once(&mut app, &ctx, width, height);
                    app.explore.confirm_delete = None;
                    app.explore.compare_a = Some(999);
                    draw_once(&mut app, &ctx, width, height);
                }
                if with_data {
                    // The saved signal followed its frames, and the form's trial one too.
                    let snap = lock(&app.snapshot).clone();
                    assert!(snap.signal_traces[0].value.is_some());
                }
            }
        }
    }

    #[test]
    fn the_sd_card_screen_draws_in_every_state_at_every_size() {
        use crate::sdcard::{parse_listing, parse_status, Upload};
        let states = [
            "state nocard\nmessage No card found\n",
            "state nofs\nmessage The card has no file system the unit can read. It can be formatted.\n",
            "state toobig\nmessage This card holds 31.9 GB; only cards up to 16 GB are supported\n",
            "state ejected\nmessage Ejected. It is safe to take the card out.\n",
            "state formatting\nbusy format\n",
            "state ready\ncard_name SD16G\ncard_type SDHC\ncard_bytes 15931539456\ntotal_bytes 15900000000\nfree_bytes 14000000000\nspace_known 1\nclock 1\nheap_free 90000\n",
            "state ready\ncard_name SD16G\ncard_type SDHC\ncard_bytes 15931539456\nspace_known 0\nrecording 1\nrecord_wanted 1\nrecord_file /recordings/rec-0001.log\nrecord_frames 120000\nrecord_bytes 6000000\nrecord_lost 4\nauto 1\n",
            "state ready\ncard_bytes 15931539456\ntotal_bytes 15900000000\nfree_bytes 100\nspace_known 1\nbusy upload\nbusy_done 5000\nbusy_total 90000\nrecord_wanted 1\n",
        ];
        let folder = "path /recordings\nd\told stuff\t0\t2026-11-01 10:00:00\nf\trec-20261119-214501.log\t6000000\t2026-11-19 21:45:01\nf\ta very long file name that keeps going and going.log\t12\t\nmore 1\n";
        for (width, height) in [(360.0, 700.0), (390.0, 1500.0), (640.0, 800.0), (900.0, 700.0), (1180.0, 860.0)] {
            for state in states {
                for listing in [None, Some("path /\n"), Some(folder)] {
                    let ctx = egui::Context::default();
                    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                    let mut app = MonitorApp::new(&cc);
                    app.screen = Screen::SdCard;
                    app.sd.available = true;
                    app.sd.status = parse_status(state);
                    app.sd.path = "/recordings".into();
                    app.sd.listing = listing.map(parse_listing);
                    for _ in 0..3 {
                        draw_once(&mut app, &ctx, width, height);
                    }
                    // The notes, the progress bar, the typing checks and each question.
                    app.sd.note = Some(("The card is full.".into(), true));
                    app.sd.unreachable = true;
                    app.sd.new_folder = "bad/name".into();
                    app.sd.upload = Upload::Sending { name: "big.bin".into(), done: 10, total: 100 };
                    draw_once(&mut app, &ctx, width, height);
                    app.sd.upload = Upload::Idle;
                    app.sd.note = Some(("Deleted a.log.".into(), false));
                    app.sd.new_folder = "ok name".into();
                    for confirm in [
                        sdscreen::Confirm::Format,
                        sdscreen::Confirm::Eject,
                        sdscreen::Confirm::Delete { path: "/recordings/old stuff".into(), dir: true },
                        sdscreen::Confirm::Delete { path: "/a.log".into(), dir: false },
                    ] {
                        app.sd.confirm = Some(confirm);
                        draw_once(&mut app, &ctx, width, height);
                    }
                }
            }
        }
    }

    #[test]
    fn record_goes_to_the_card_only_when_this_page_is_on_the_units_own_bus_and_the_card_is_ready() {
        use crate::sdcard::parse_status;
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = MonitorApp::new(&cc);
        let unit = || Some("ws://192.168.4.1/ws".to_string());
        app.sd.available = true;
        app.sd.status = parse_status("state ready\n");
        app.link = Link::Can;
        app.selected_port = Some("ws://192.168.4.1/ws".into());
        assert!(app.record_to_card_with(unit()), "the unit's own bus with a ready card");
        assert!(!app.record_to_card_with(None), "a computer with a CANable has no unit");
        assert!(!app.record_to_card_with(Some("ws://10.0.0.9/ws".into())), "another address is not this unit");
        app.selected_port = Some("ws://10.0.0.9/ws".into());
        assert!(!app.record_to_card_with(unit()), "connected to some other address");
        app.selected_port = Some("ws://192.168.4.1/ws".into());
        for link in [Link::Idle, Link::Sim, Link::Replay] {
            app.link = link;
            assert!(!app.record_to_card_with(unit()), "not the live bus");
        }
        app.link = Link::Can;
        for status in ["state nocard\n", "state nofs\n", "state toobig\n", "state ejected\n", "state formatting\nbusy format\n", "state ready\nbusy format\n"] {
            app.sd.status = parse_status(status);
            assert!(!app.record_to_card_with(unit()), "{status:?}");
        }
        app.sd.status = parse_status("state ready\n");
        app.sd.unreachable = true;
        assert!(!app.record_to_card_with(unit()), "the unit is not answering");
        app.sd.unreachable = false;
        app.sd.available = false;
        assert!(!app.record_to_card_with(unit()), "the Atomic CAN Base unit has no card");
    }

    #[test]
    fn the_frames_screen_draws_its_record_row_in_every_card_state_at_every_size() {
        use crate::sdcard::parse_status;
        let states = [
            "state ready\n",
            "state nocard\n",
            "state ready\nrecording 1\nrecord_wanted 1\nrecord_file /recordings/rec-20261119-214501.log\nrecord_frames 5000\nrecord_bytes 300000\nrecord_lost 2\n",
            "state ready\nrecord_wanted 1\n",
        ];
        for (width, height) in [(360.0, 700.0), (390.0, 1500.0), (640.0, 800.0), (1180.0, 860.0)] {
            for link in [Link::Idle, Link::Can, Link::Replay, Link::Sim] {
                for state in states {
                    for saved in [None, Some("/recordings/rec-0001.log".to_string())] {
                        let ctx = egui::Context::default();
                        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                        let mut app = MonitorApp::new(&cc);
                        app.screen = Screen::Frames;
                        app.link = link;
                        app.sd.available = true;
                        app.sd.status = parse_status(state);
                        app.sd.saved = saved;
                        for _ in 0..2 {
                            draw_once(&mut app, &ctx, width, height);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_sd_card_entry_is_in_the_menus_only_when_the_unit_has_a_card() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = MonitorApp::new(&cc);
        assert!(!app.sd.available);
        assert!(tab_lit(Screen::SdCard, Screen::More));
        for width in [390.0, 1180.0] {
            app.screen = Screen::More;
            draw_once(&mut app, &ctx, width, 900.0);
            app.sd.available = true;
            draw_once(&mut app, &ctx, width, 900.0);
            app.sd.available = false;
        }
    }

    #[test]
    fn the_event_log_screen_draws_its_records_at_every_size() {
        for (width, height) in [(360.0, 700.0), (640.0, 800.0), (1180.0, 860.0)] {
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let mut app = MonitorApp::new(&cc);
            app.screen = Screen::Events;
            app.events_on = true;
            app.event_rows = (0..30u32)
                .rev()
                .map(|number| {
                    [
                        "26/11/09 22:29:50".into(),
                        "0x1010".into(),
                        "0FF00FE800150706".into(),
                        "Off".into(),
                        number.to_string(),
                        "30 0A 1A 0B 09 16 1D 32 | 00 00 22 10 00 00 00 00 | 00 00 07 07 00 00 00 00".into(),
                    ]
                })
                .collect();
            for _ in 0..2 {
                draw_once(&mut app, &ctx, width, height);
            }
        }
    }

    #[test]
    fn the_battery_clock_card_draws_in_every_state_at_every_size() {
        use crate::clock::Moment;
        let battery = |year, month, day, hour, minute, second| {
            Some(Moment { year, month, day, hour, minute, second })
        };
        for (width, height) in [(360.0, 800.0), (640.0, 800.0), (1180.0, 860.0)] {
            for link in [Link::Idle, Link::Sim, Link::Replay, Link::Can] {
                for clock in [None, battery(2001, 1, 18, 11, 30, 30), battery(2026, 11, 9, 22, 28, 4)] {
                    let ctx = egui::Context::default();
                    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                    let mut app = MonitorApp::new(&cc);
                    app.screen = Screen::Parameters;
                    app.link = link;
                    lock(&app.snapshot).battery_clock = clock;
                    lock(&app.snapshot).clock_note = clock::NOTE_MATCHES.into();
                    draw_once(&mut app, &ctx, width, height);
                    // With the confirmation open, and with the other notes.
                    app.confirm_clock = true;
                    lock(&app.snapshot).clock_note = clock::NOTE_DIFFERS.into();
                    draw_once(&mut app, &ctx, width, height);
                    app.confirm_clock = false;
                    app.clock_set_at = Some(Instant::now());
                    draw_once(&mut app, &ctx, width, height);
                }
            }
        }
    }

    #[test]
    fn confirming_sends_one_clock_request_and_only_on_a_live_bus() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = MonitorApp::new(&cc);
        let (tx, rx) = mpsc::channel();
        app.ask_tx = Some(tx);
        // No bus: nothing is sent.
        app.link = Link::Idle;
        app.send_clock();
        assert!(rx.try_recv().is_err());
        assert_eq!(lock(&app.snapshot).clock_note, "Connect an adapter to set the battery clock.");
        // A simulation or a replay has no battery either.
        for link in [Link::Sim, Link::Replay] {
            app.link = link;
            app.send_clock();
            assert!(rx.try_recv().is_err());
        }
        // A live bus: one request, for this device's time now.
        app.link = Link::Can;
        app.send_clock();
        match rx.try_recv() {
            Ok(Ask::SetClock(moment)) => assert!(moment.is_valid() && moment.year >= 2026, "{moment:?}"),
            other => panic!("expected one clock request, got {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "only one");
        assert_eq!(lock(&app.snapshot).clock_note, clock::NOTE_SETTING);
        assert!(app.clock_set_at.is_some(), "the button is held for a moment after");
    }

    #[test]
    fn the_set_flags_card_spans_the_width_like_the_other_cards() {
        let mut snapshot = Snapshot::default();
        crate::decode::apply(
            &mut snapshot,
            &crate::snapshot::Frame::new(0x184, &[0x45, 0x82, 0x01, 0x00, 0x04, 0x0A, 0x00, 0x00]),
        );
        assert!(!explain::set_flags(&snapshot).is_empty(), "the test needs some flags set");
        let available = 1400.0;
        let mut width = 0.0;
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
        let ctx = egui::Context::default();
        let output = ctx.run_pass(input, |ctx| {
            apply_theme(ctx);
            let layer = egui::LayerId::new(egui::Order::Background, egui::Id::unique("flags-test"));
            let mut ui = egui::Ui::new(
                ctx.clone(),
                egui::Id::unique("flags-test-ui"),
                egui::UiBuilder::new()
                    .layer_id(layer)
                    .max_rect(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(available, 1000.0)))
                    .layout(Layout::top_down(Align::Min)),
            );
            let mut detail = None;
            set_flags(&mut ui, &snapshot, &mut detail);
            width = ui.min_rect().width();
        });
        output.drop_without_applying_deltas();
        assert!(width >= available - 1.0, "the card is {width} wide in a space {available} wide");
    }

    #[test]
    fn the_version_is_three_numbers_taken_from_the_workspace() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{VERSION}");
        assert!(parts.iter().all(|part| part.parse::<u32>().is_ok()), "{VERSION}");
    }

    #[test]
    fn the_width_decides_the_form_of_the_window() {
        assert_eq!(Form::for_width(360.0), Form::Phone);
        assert_eq!(Form::for_width(390.0), Form::Phone);
        assert_eq!(Form::for_width(599.9), Form::Phone);
        assert_eq!(Form::for_width(600.0), Form::Medium);
        assert_eq!(Form::for_width(812.0), Form::Medium);
        assert_eq!(Form::for_width(899.9), Form::Medium);
        assert_eq!(Form::for_width(900.0), Form::Wide);
        assert_eq!(Form::for_width(1180.0), Form::Wide);
    }

    #[test]
    fn the_more_tab_stays_lit_on_the_screens_it_leads_to() {
        assert!(tab_lit(Screen::Live, Screen::Live));
        assert!(!tab_lit(Screen::Live, Screen::Readings));
        assert!(tab_lit(Screen::More, Screen::More));
        assert!(tab_lit(Screen::Parameters, Screen::More));
        assert!(tab_lit(Screen::Events, Screen::More));
        assert!(tab_lit(Screen::Explore, Screen::More));
        assert!(!tab_lit(Screen::Parameters, Screen::Live));
        assert!(!tab_lit(Screen::Flags, Screen::More));
    }

    #[test]
    fn the_header_shows_the_last_three_digits_of_the_pack_id() {
        assert_eq!(pack_label(None), "Pack —");
        assert_eq!(pack_label(Some("2713969177")), "Pack …177");
        assert_eq!(pack_label(Some("42")), "Pack 42");
        assert_eq!(pack_label(Some("123")), "Pack 123");
    }

    #[test]
    fn split_export_covers_every_panel() {
        let ctx = egui::Context::default();
        let width = 640.0;
        let mut image = None;
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 4_000.0)));
        let output = ctx.run_pass(input, |ctx| {
            apply_theme(ctx);
            assert_eq!(ctx.pixels_per_point(), 1.0);
            image = Some(render_chart_image(
                ctx,
                width,
                true,
                1,
                &[],
                &(0..SERIES.len()).collect::<Vec<_>>(),
                "",
                &[],
            ));
        });
        output.drop_without_applying_deltas();
        let (image_w, image_h, rgba) = image.expect("chart image");
        assert_eq!(image_w, width as u32, "image is {image_w}x{image_h}");
        assert!(image_h >= 7 * 168, "split image height {image_h}");
        assert_eq!(rgba.len(), image_w as usize * image_h as usize * 4);
    }
    fn replaying_app() -> (egui::Context, MonitorApp) {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = MonitorApp::new(&cc);
        app.link = Link::Replay;
        lock(&app.snapshot).replay = Some(crate::snapshot::ReplayState {
            name: "test.log".into(),
            position_ms: 5_000,
            length_ms: 60_000,
            paused: true,
            speed: 1.0,
            finished: false,
            source_note: String::new(),
            seek_to: None,
        });
        (ctx, app)
    }

    fn draw_with_keys(app: &mut MonitorApp, ctx: &egui::Context, keys: &[egui::Key]) {
        let mut frame = eframe::Frame::_new_kittest();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0)));
        for key in keys {
            input.events.push(egui::Event::Key {
                key: *key,
                physical_key: Some(*key),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        app.sync_signals();
        let output = ctx.run_ui(input, |ui| {
            apply_theme(ui.ctx());
            eframe::App::ui(app, ui, &mut frame);
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn the_replay_bar_draws_on_every_screen_at_every_size() {
        for (width, height) in [(360.0, 700.0), (390.0, 1500.0), (640.0, 800.0), (900.0, 700.0), (1180.0, 860.0)] {
            for screen in [
                Screen::Live,
                Screen::Readings,
                Screen::Flags,
                Screen::Frames,
                Screen::Explore,
                Screen::Parameters,
                Screen::Events,
                Screen::More,
            ] {
                for finished in [false, true] {
                    let (ctx, mut app) = replaying_app();
                    app.screen = screen;
                    if finished {
                        let mut snap = lock(&app.snapshot);
                        let state = snap.replay.as_mut().unwrap();
                        state.finished = true;
                        state.position_ms = state.length_ms;
                    }
                    for _ in 0..3 {
                        draw_once(&mut app, &ctx, width, height);
                    }
                }
            }
        }
    }

    #[test]
    fn the_arrow_keys_step_a_paused_replay_and_space_toggles_it() {
        let (ctx, mut app) = replaying_app();
        draw_with_keys(&mut app, &ctx, &[]);
        draw_with_keys(&mut app, &ctx, &[egui::Key::ArrowRight]);
        {
            let snap = lock(&app.snapshot);
            let state = snap.replay.as_ref().unwrap();
            assert_eq!(state.seek_to, Some(5_100), "one 100 ms step on");
            assert!(state.paused);
        }
        lock(&app.snapshot).replay.as_mut().unwrap().seek_to = None;
        draw_with_keys(&mut app, &ctx, &[egui::Key::ArrowLeft]);
        assert_eq!(lock(&app.snapshot).replay.as_ref().unwrap().seek_to, Some(4_900));
        lock(&app.snapshot).replay.as_mut().unwrap().seek_to = None;
        draw_with_keys(&mut app, &ctx, &[egui::Key::Space]);
        assert!(!lock(&app.snapshot).replay.as_ref().unwrap().paused, "space plays");
        draw_with_keys(&mut app, &ctx, &[egui::Key::Space]);
        assert!(lock(&app.snapshot).replay.as_ref().unwrap().paused, "space pauses again");
    }

    #[test]
    fn stepping_is_held_while_a_recording_is_being_written() {
        let (ctx, mut app) = replaying_app();
        let path = std::env::temp_dir().join("s01-transport-test.log");
        let recorder = crate::capture::Recorder::create(&path, "test", 1_793_448_180_000).unwrap();
        lock(&app.snapshot).recorder = Some(Arc::new(Mutex::new(recorder)));
        draw_with_keys(&mut app, &ctx, &[]);
        draw_with_keys(&mut app, &ctx, &[egui::Key::ArrowRight]);
        assert_eq!(lock(&app.snapshot).replay.as_ref().unwrap().seek_to, None);
        lock(&app.snapshot).recorder = None;
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn two_steps_before_the_replay_catches_up_add_up() {
        let (ctx, mut app) = replaying_app();
        draw_with_keys(&mut app, &ctx, &[]);
        draw_with_keys(&mut app, &ctx, &[egui::Key::ArrowRight]);
        draw_with_keys(&mut app, &ctx, &[egui::Key::ArrowRight]);
        draw_with_keys(&mut app, &ctx, &[egui::Key::ArrowLeft]);
        assert_eq!(lock(&app.snapshot).replay.as_ref().unwrap().seek_to, Some(5_100));
    }
    #[test]
    fn the_debug_switch_draws_on_frames_and_flags_in_every_state_and_size() {
        use crate::snapshot::DebugState;
        for (width, height) in [(360.0, 700.0), (640.0, 800.0), (1180.0, 860.0)] {
            for screen in [Screen::Frames, Screen::Flags] {
                for link in [Link::Idle, Link::Can] {
                    for wanted in [false, true] {
                        for state in [
                            DebugState::Idle,
                            DebugState::Sent,
                            DebugState::Confirmed,
                            DebugState::AlreadyOn,
                            DebugState::Unconfirmed,
                        ] {
                            let ctx = egui::Context::default();
                            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                            let mut app = MonitorApp::new(&cc);
                            app.screen = screen;
                            app.link = link;
                            {
                                let mut snap = lock(&app.snapshot);
                                snap.debug_wanted = wanted;
                                snap.debug = state;
                            }
                            for _ in 0..2 {
                                draw_once(&mut app, &ctx, width, height);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_state_of_debug_mode_has_a_note_and_only_idle_with_the_switch_off_has_none() {
        use crate::snapshot::DebugState;
        assert!(debug_note(false, DebugState::Idle).is_none());
        assert!(debug_note(true, DebugState::Idle).is_some(), "switched on, waiting for the scooter");
        for state in [DebugState::Sent, DebugState::Confirmed, DebugState::AlreadyOn, DebugState::Unconfirmed] {
            assert!(debug_note(true, state).is_some(), "{state:?}");
        }
    }
    #[test]
    fn a_recording_left_in_the_browser_is_offered_on_every_screen_at_every_size() {
        for (width, height) in [(360.0, 700.0), (640.0, 800.0), (1180.0, 860.0)] {
            for screen in [Screen::Live, Screen::Frames, Screen::Flags, Screen::Parameters] {
                let ctx = egui::Context::default();
                let cc = eframe::CreationContext::_new_kittest(ctx.clone());
                let mut app = MonitorApp::new(&cc);
                app.screen = screen;
                app.link = Link::Can;
                app.recoverable = vec![
                    platform::Recoverable { id: "rec-1".into(), name: "recording-a.log".into(), bytes: 12_345_678 },
                    platform::Recoverable { id: "rec-2".into(), name: "recording-b.log".into(), bytes: 900 },
                ];
                for _ in 0..2 {
                    draw_once(&mut app, &ctx, width, height);
                }
            }
        }
    }

    #[test]
    fn sizes_are_shown_in_kilobytes_and_megabytes() {
        assert_eq!(size_text(0), "0 KB");
        assert_eq!(size_text(900), "1 KB");
        assert_eq!(size_text(340_100), "341 KB");
        assert_eq!(size_text(1_000_000), "1.0 MB");
        assert_eq!(size_text(12_345_678), "12.3 MB");
    }

    #[test]
    fn what_the_browser_says_about_a_save_goes_on_the_screen_with_the_counts() {
        let mut app = app_for_tests();
        app.pending_save = Some((120, 2));
        app.show_saved(vec!["your downloads as recording-x.log".into()]);
        assert_eq!(app.record_note, "Saved 120 frames and 2 markers to your downloads as recording-x.log");
        assert!(app.pending_save.is_none());
        // A recovered recording has no counts.
        app.show_saved(vec!["your downloads as recording-y.log".into()]);
        assert_eq!(app.record_note, "Recording saved to your downloads as recording-y.log");
    }
    #[test]
    fn the_replay_bar_shows_the_experimental_note_for_another_tools_recording() {
        for (width, height) in [(360.0, 700.0), (900.0, 700.0)] {
            let (ctx, mut app) = replaying_app();
            lock(&app.snapshot).replay.as_mut().unwrap().source_note =
                "Experimental: read as candump log (candump -L). Channel can0 used, of 2. Left out: 3 29-bit identifier, 1 on another channel.".into();
            for _ in 0..2 {
                draw_once(&mut app, &ctx, width, height);
            }
        }
    }
}
