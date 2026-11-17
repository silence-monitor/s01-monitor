//! The Explore screen: two tools for working out what an undecoded frame
//! means.
//!
//! **Signals** read any bytes of any frame as a number and draw them live, so
//! a guess can be tried while the scooter does something (press the horn, turn
//! the throttle) and kept if it is right.
//!
//! **Compare** takes two markers and lists the bytes that differ between what
//! the bus said at the first and at the second.

use std::collections::VecDeque;

use eframe::egui::{self, Button, Color32, CornerRadius, RichText, Sense, Stroke, TextEdit, Vec2};

use super::{
    action_button, card, chip, control_row, MonitorApp, Screen, ALARM, CHART_BG, GOOD, INK, LINE_COLOR, MUTED, WARN,
};
use crate::compare::{self, Diff};
use crate::export::Table;
use crate::frames;
use crate::signals::{SignalDef, Trace, MAX_SIGNALS};
use crate::snapshot::{self, Snapshot};
use crate::views;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Signals,
    Compare,
}

/// A signal being tried, as the person typed it. Numbers stay as text until
/// they are used, so half-typed input is never in the way.
#[derive(Clone, Debug, PartialEq)]
pub struct SignalDraft {
    pub name: String,
    pub id: String,
    pub start: u8,
    pub count: u8,
    pub signed: bool,
    pub big_endian: bool,
    pub scale: String,
    pub offset: String,
    pub unit: String,
}

impl Default for SignalDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            id: String::new(),
            start: 0,
            count: 1,
            signed: false,
            big_endian: false,
            scale: "1".into(),
            offset: "0".into(),
            unit: String::new(),
        }
    }
}

/// An identifier typed in hex, with or without `0x`.
pub fn parse_id(text: &str) -> Option<u16> {
    let text = text.trim();
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    u16::from_str_radix(digits, 16).ok().filter(|id| *id <= 0x7FF)
}

fn parse_number(text: &str) -> Option<f32> {
    text.trim().replace(',', ".").parse::<f32>().ok().filter(|number| number.is_finite())
}

impl SignalDraft {
    /// The signal as typed. With `preview` an empty name becomes "Draft", so a
    /// guess can be watched before it is named.
    pub fn to_def(&self, preview: bool) -> Result<SignalDef, String> {
        let id = parse_id(&self.id).ok_or("Type the identifier in hex, for example 182 or 0x182.")?;
        let scale = parse_number(&self.scale).ok_or("The scale must be a number, for example 0.1.")?;
        let offset = parse_number(&self.offset).ok_or("The offset must be a number, for example 0 or -40.")?;
        let mut def = SignalDef {
            key: String::new(),
            name: if preview && self.name.trim().is_empty() { "Draft".into() } else { self.name.clone() },
            id,
            start: self.start,
            count: self.count,
            signed: self.signed,
            big_endian: self.big_endian,
            scale,
            offset,
            unit: self.unit.clone(),
        };
        def.tidy();
        def.check()?;
        Ok(def)
    }

    /// Keeps the bytes inside the frame: a frame has bytes 0 to 7.
    pub fn fit(&mut self) {
        self.start = self.start.min(7);
        self.count = self.count.clamp(1, 4).min(8 - self.start);
    }
}

/// What the Explore screen remembers between frames.
#[derive(Debug)]
pub struct ExploreState {
    pub tab: Tab,
    pub draft: SignalDraft,
    /// How many minutes of history the signal plots show.
    pub window_min: u32,
    /// A saved signal the person asked to delete: its key and name.
    pub confirm_delete: Option<(String, String)>,
    /// The name of a signal just sent to be saved, so the form can be emptied
    /// once it has been.
    pub just_saved: Option<String>,
    /// The two markers being compared. `None` means the newest two.
    pub compare_a: Option<usize>,
    pub compare_b: Option<usize>,
    pub hide_noisy: bool,
}

impl Default for ExploreState {
    fn default() -> Self {
        Self {
            tab: Tab::Signals,
            draft: SignalDraft::default(),
            window_min: 1,
            confirm_delete: None,
            just_saved: None,
            compare_a: None,
            compare_b: None,
            hide_noisy: true,
        }
    }
}

/// A reading in as few characters as make sense.
pub fn number_text(value: f32) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let magnitude = value.abs();
    let text = if magnitude >= 1000.0 {
        format!("{value:.0}")
    } else if magnitude >= 100.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.3}")
    };
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text };
    if text == "-0" {
        "0".into()
    } else {
        text
    }
}

/// "bit 2" or "bits 0, 3, 7" for the bits set in a byte.
pub fn bit_list(bits: u8) -> String {
    let set: Vec<String> = (0..8).filter(|bit| bits & (1 << bit) != 0).map(|bit| bit.to_string()).collect();
    match set.len() {
        0 => String::new(),
        1 => format!("bit {}", set[0]),
        _ => format!("bits {}", set.join(", ")),
    }
}

/// The comparison as a table that can be saved.
pub fn compare_table(diffs: &[Diff], a_name: &str, b_name: &str) -> Table {
    let hex = |byte: Option<u8>| byte.map_or_else(String::new, |byte| format!("{byte:02X}"));
    Table {
        headers: ["Identifier", "Name", "Byte", a_name, b_name, "Bits changed"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        rows: diffs
            .iter()
            .map(|diff| {
                let name = frames::label(diff.id);
                vec![
                    format!("0x{:03X}", diff.id),
                    if name.is_empty() { "Not decoded".to_string() } else { name.to_string() },
                    diff.byte.to_string(),
                    hex(diff.a),
                    hex(diff.b),
                    diff.bits().map_or_else(String::new, bit_list),
                ]
            })
            .collect(),
    }
}

/// A plot's line as fractions of its box: x from the left, y up from the
/// bottom. Values hold between readings, so the line goes across and then up
/// or down. Also the lowest and highest value drawn.
pub type PlotShape = (Vec<(f32, f32)>, f32, f32);

pub fn plot_shape(points: &VecDeque<(u64, f32)>, window_ms: u64) -> Option<PlotShape> {
    let end = points.back()?.0;
    let start = end.saturating_sub(window_ms);
    let visible: Vec<(u64, f32)> = points.iter().filter(|(at, _)| *at >= start).copied().collect();
    let low = visible.iter().map(|(_, value)| *value).fold(f32::INFINITY, f32::min);
    let high = visible.iter().map(|(_, value)| *value).fold(f32::NEG_INFINITY, f32::max);
    let (floor, ceiling) = if high - low < 1e-6 { (low - 1.0, high + 1.0) } else { (low, high) };
    let across = |at: u64| (at - start) as f32 / window_ms.max(1) as f32;
    let up = |value: f32| (value - floor) / (ceiling - floor);
    let mut line: Vec<(f32, f32)> = Vec::with_capacity(visible.len() * 2);
    for (at, value) in &visible {
        if let Some(&(_, before)) = line.last() {
            line.push((across(*at), before));
        }
        line.push((across(*at), up(*value)));
    }
    Some((line, low, high))
}

fn plot(ui: &mut egui::Ui, points: &VecDeque<(u64, f32)>, window_ms: u64, height: f32, color: Color32) {
    let width = ui.available_width().max(60.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), CHART_BG);
    painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, LINE_COLOR), egui::StrokeKind::Inside);
    let Some((line, low, high)) = plot_shape(points, window_ms) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No readings yet",
            egui::FontId::proportional(12.0),
            MUTED,
        );
        return;
    };
    let inner = rect.shrink2(Vec2::new(10.0, 14.0));
    let screen: Vec<egui::Pos2> = line
        .iter()
        .map(|(x, y)| egui::pos2(inner.left() + x * inner.width(), inner.bottom() - y * inner.height()))
        .collect();
    painter.line(screen, Stroke::new(1.5, color));
    let small = egui::FontId::proportional(11.0);
    painter.text(rect.left_top() + Vec2::new(6.0, 2.0), egui::Align2::LEFT_TOP, number_text(high), small.clone(), MUTED);
    painter.text(rect.left_bottom() + Vec2::new(6.0, -2.0), egui::Align2::LEFT_BOTTOM, number_text(low), small, MUTED);
}

/// The eight bytes of a frame, the ones being read in green.
fn bytes_job(trace: &Trace, start: u8, count: u8) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    for index in 0..8usize {
        let reading = index >= usize::from(start) && index < usize::from(start) + usize::from(count);
        let (text, color) = if index < trace.len {
            (format!("{:02X} ", trace.bytes[index]), if reading { GOOD } else { MUTED })
        } else {
            ("-- ".to_string(), MUTED)
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

fn age_note(snap: &Snapshot, trace: &Trace, id: u16) -> String {
    if trace.raw.is_none() {
        return format!("Waiting for a frame from 0x{id:03X}…");
    }
    let now = if snap.replay.is_some() { snap.explorer.now_ms } else { snapshot::now_ms() };
    let age = now.saturating_sub(trace.last_ms);
    if age < 2_000 {
        "Updating".into()
    } else {
        format!("Last frame {} s ago", age / 1000)
    }
}

const SIGNAL_COLORS: [Color32; 4] = [
    GOOD,
    Color32::from_rgb(0x7e, 0xb8, 0xd4),
    Color32::from_rgb(0xe2, 0xb1, 0x5a),
    Color32::from_rgb(0xc4, 0x7c, 0x9a),
];

impl MonitorApp {
    /// Tells the snapshot which signals to read: the saved ones, and the one
    /// being tried while the form is on screen.
    pub(super) fn sync_signals(&mut self) {
        let mut defs = self.signal_defs.clone();
        if self.screen == Screen::Explore && self.explore.tab == Tab::Signals {
            if let Ok(mut draft) = self.explore.draft.to_def(true) {
                draft.key = "draft".into();
                defs.push(draft);
            }
        }
        snapshot::lock(&self.snapshot).set_signals(&defs);
    }

    pub(super) fn explore(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let phone = self.phone();
        control_row(ui, phone, |ui| {
            for (label, tab) in [("Signals", Tab::Signals), ("Compare two moments", Tab::Compare)] {
                if chip(ui, label, self.explore.tab == tab).clicked() {
                    self.explore.tab = tab;
                    self.view_note = None;
                }
            }
        });
        ui.add_space(10.0);
        match self.explore.tab {
            Tab::Signals => self.signals_tab(ui, snap),
            Tab::Compare => self.compare_tab(ui, snap),
        }
        ui.add_space(12.0);
    }

    fn signals_tab(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.label(
            RichText::new(
                "Read bytes of any frame as a number and watch them move. Try a guess while the scooter does \
                 something, then keep it if the line follows.",
            )
            .size(12.0)
            .color(MUTED),
        );
        ui.add_space(6.0);
        control_row(ui, self.phone(), |ui| {
            ui.label(RichText::new("Show the last").color(MUTED));
            for minutes in [1u32, 5, 15] {
                if chip(ui, &format!("{minutes} min"), self.explore.window_min == minutes).clicked() {
                    self.explore.window_min = minutes;
                }
            }
        });
        ui.add_space(8.0);
        if let Some((text, bad)) = &self.view_note {
            ui.label(RichText::new(text).size(12.0).color(if *bad { ALARM } else { GOOD }));
            ui.add_space(6.0);
        }

        let window_ms = u64::from(self.explore.window_min) * 60_000;
        let saved = self.signal_defs.clone();
        let mut ask_delete = None;
        if saved.is_empty() {
            ui.label(RichText::new("No signals saved yet.").color(MUTED));
            ui.add_space(8.0);
        }
        for (index, def) in saved.iter().enumerate() {
            let trace = snap.signal_traces.get(index).cloned().unwrap_or_default();
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&def.name).size(16.0).color(INK));
                    ui.label(RichText::new(def.describe()).size(12.0).color(MUTED));
                });
                ui.horizontal_wrapped(|ui| {
                    let value = trace.value.map_or_else(|| "—".to_string(), number_text);
                    let unit = if def.unit.is_empty() { String::new() } else { format!(" {}", def.unit) };
                    ui.label(RichText::new(format!("{value}{unit}")).size(22.0).color(INK));
                    if let Some(raw) = trace.raw {
                        ui.label(RichText::new(format!("raw {raw}")).monospace().size(12.0).color(MUTED));
                    }
                });
                ui.label(bytes_job(&trace, def.start, def.count));
                ui.label(RichText::new(age_note(snap, &trace, def.id)).size(12.0).color(MUTED));
                ui.add_space(4.0);
                plot(ui, &trace.history, window_ms, if self.phone() { 70.0 } else { 90.0 }, SIGNAL_COLORS[index % 4]);
                ui.add_space(4.0);
                if ui.add(Button::new(RichText::new("Delete").color(ALARM))).clicked() {
                    ask_delete = Some((def.key.clone(), def.name.clone()));
                }
            });
            ui.add_space(8.0);
        }
        if ask_delete.is_some() {
            self.explore.confirm_delete = ask_delete;
        }

        self.signal_form(ui, snap, saved.len());
    }

    fn signal_form(&mut self, ui: &mut egui::Ui, snap: &Snapshot, saved_count: usize) {
        let mut draft = self.explore.draft.clone();
        let phone = self.phone();
        let seen: Vec<(u16, bool)> = snap.explorer.ids.iter().map(|(id, stats)| (*id, stats.decoded)).collect();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("New signal").size(16.0).color(INK));
            ui.add_space(6.0);
            control_row(ui, phone, |ui| {
                ui.label(RichText::new("Name").color(MUTED));
                ui.add(TextEdit::singleline(&mut draft.name).hint_text("e.g. Horn").desired_width(150.0));
                ui.label(RichText::new("Unit").color(MUTED));
                ui.add(TextEdit::singleline(&mut draft.unit).hint_text("optional").desired_width(70.0));
            });
            control_row(ui, phone, |ui| {
                ui.label(RichText::new("Identifier (hex)").color(MUTED));
                ui.add(TextEdit::singleline(&mut draft.id).hint_text("182").desired_width(60.0));
                egui::ComboBox::from_id_salt("signal-seen")
                    .selected_text("Pick one seen on the bus")
                    .show_ui(ui, |ui| {
                        if seen.is_empty() {
                            ui.label(RichText::new("Nothing seen yet. Connect or replay a capture.").color(MUTED));
                        }
                        for (id, decoded) in &seen {
                            let note = if *decoded { "" } else { "  not decoded" };
                            if ui.selectable_label(false, format!("0x{id:03X}{note}")).clicked() {
                                draft.id = format!("{id:X}");
                            }
                        }
                    });
            });
            control_row(ui, phone, |ui| {
                ui.label(RichText::new("First byte").color(MUTED));
                ui.add(egui::DragValue::new(&mut draft.start).range(0..=7));
                ui.label(RichText::new("How many bytes").color(MUTED));
                ui.add(egui::DragValue::new(&mut draft.count).range(1..=4));
                ui.checkbox(&mut draft.signed, "Can be negative");
            });
            draft.fit();
            if draft.count > 1 {
                control_row(ui, phone, |ui| {
                    ui.label(RichText::new("Byte order").color(MUTED));
                    if chip(ui, "Last byte is the biggest part", !draft.big_endian).clicked() {
                        draft.big_endian = false;
                    }
                    if chip(ui, "First byte is the biggest part", draft.big_endian).clicked() {
                        draft.big_endian = true;
                    }
                });
            }
            control_row(ui, phone, |ui| {
                ui.label(RichText::new("Reading = number ×").color(MUTED));
                ui.add(TextEdit::singleline(&mut draft.scale).desired_width(70.0));
                ui.label(RichText::new("+").color(MUTED));
                ui.add(TextEdit::singleline(&mut draft.offset).desired_width(70.0));
            });
            ui.add_space(6.0);

            let preview = snap.signal_traces.get(saved_count).cloned().unwrap_or_default();
            match draft.to_def(true) {
                Ok(def) => {
                    ui.label(bytes_job(&preview, def.start, def.count));
                    let value = preview.value.map_or_else(|| "—".to_string(), number_text);
                    let unit = if def.unit.is_empty() { String::new() } else { format!(" {}", def.unit) };
                    ui.label(
                        RichText::new(format!("Reads {value}{unit}. {}", age_note(snap, &preview, def.id)))
                            .color(INK),
                    );
                    ui.add_space(4.0);
                    let window_ms = u64::from(self.explore.window_min) * 60_000;
                    plot(ui, &preview.history, window_ms, if phone { 70.0 } else { 90.0 }, WARN);
                }
                Err(problem) => {
                    let text = if draft.id.trim().is_empty() {
                        "Pick an identifier, or type one, to start watching its bytes.".to_string()
                    } else {
                        problem
                    };
                    ui.label(RichText::new(text).size(12.0).color(MUTED));
                }
            }
            ui.add_space(8.0);
            let named = draft.to_def(false);
            let room = saved_count < MAX_SIGNALS;
            let can_save = named.is_ok() && room && self.explore.just_saved.is_none();
            if action_button(ui, "Save signal", can_save).clicked() {
                if let Ok(def) = &named {
                    self.explore.just_saved = Some(def.name.clone());
                    self.view_note = None;
                    self.view_pending += 1;
                    views::change(views::Change::AddSignal(def.clone()));
                }
            }
            if !room {
                ui.label(RichText::new(format!("There is room for {MAX_SIGNALS} signals. Delete one first.")).size(12.0).color(WARN));
            } else if draft.name.trim().is_empty() && draft.to_def(true).is_ok() {
                ui.label(RichText::new("Give it a name to save it.").size(12.0).color(MUTED));
            } else if let Err(problem) = &named {
                if !draft.name.trim().is_empty() && !draft.id.trim().is_empty() {
                    ui.label(RichText::new(problem).size(12.0).color(WARN));
                }
            }
        });
        // The form empties once the signal has been kept.
        if let Some(name) = self.explore.just_saved.clone() {
            if self.signal_defs.iter().any(|def| def.name == name) {
                draft.name.clear();
                self.explore.just_saved = None;
            } else if self.view_pending == 0 {
                self.explore.just_saved = None;
            }
        }
        self.explore.draft = draft;
    }

    /// Asks before a saved signal is deleted, and deletes it on a yes.
    pub(super) fn signal_dialog(&mut self, ctx: &egui::Context) {
        let Some((key, name)) = self.explore.confirm_delete.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::unique("delete-signal")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new("Delete this signal?").size(18.0).color(INK));
            ui.add_space(6.0);
            ui.label(RichText::new(format!("\"{name}\" will be removed. {}", super::DELETE_WARNING)).color(MUTED));
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
            self.explore.confirm_delete = None;
            self.view_note = None;
            self.view_pending += 1;
            views::change(views::Change::RemoveSignal { key });
        } else if cancelled || modal.should_close() {
            self.explore.confirm_delete = None;
        }
    }

    fn compare_tab(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.label(
            RichText::new(
                "A marker remembers what every identifier was saying when you dropped it. Drop one before and one \
                 after you do something (press the horn, switch the lights) and the bytes that changed show up here.",
            )
            .size(12.0)
            .color(MUTED),
        );
        ui.add_space(6.0);
        self.mark_row(ui);
        ui.add_space(10.0);
        let marks = &snap.explorer.marks;
        if marks.len() < 2 {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(if marks.is_empty() {
                        "No markers yet. Connect (or replay a capture), then press Mark."
                    } else {
                        "One marker so far. Do something on the scooter, then press Mark again."
                    })
                    .color(MUTED),
                );
            });
            return;
        }
        let last = marks.len() - 1;
        let mut a = self.explore.compare_a.filter(|index| *index <= last).unwrap_or(last - 1);
        let mut b = self.explore.compare_b.filter(|index| *index <= last).unwrap_or(last);
        let name_of = |index: usize| {
            let text = marks[index].text.as_str();
            format!("#{} {}", index + 1, if text.is_empty() { "(no note)" } else { text })
        };
        control_row(ui, self.phone(), |ui| {
            ui.label(RichText::new("From").color(MUTED));
            egui::ComboBox::from_id_salt("compare-a").selected_text(name_of(a)).show_ui(ui, |ui| {
                for index in 0..marks.len() {
                    ui.selectable_value(&mut a, index, name_of(index));
                }
            });
            ui.label(RichText::new("to").color(MUTED));
            egui::ComboBox::from_id_salt("compare-b").selected_text(name_of(b)).show_ui(ui, |ui| {
                for index in 0..marks.len() {
                    ui.selectable_value(&mut b, index, name_of(index));
                }
            });
            if ui.button("Swap").clicked() {
                std::mem::swap(&mut a, &mut b);
            }
        });
        ui.checkbox(&mut self.explore.hide_noisy, "Leave out bytes that change all the time (counters and clocks)");
        self.explore.compare_a = Some(a);
        self.explore.compare_b = Some(b);

        let all = compare::diff(&marks[a].state, &marks[b].state);
        let shown: Vec<Diff> = all
            .iter()
            .filter(|diff| {
                !(self.explore.hide_noisy
                    && snap.explorer.ids.get(&diff.id).is_some_and(|stats| stats.is_noisy(diff.byte)))
            })
            .cloned()
            .collect();
        let left_out = all.len() - shown.len();
        ui.add_space(6.0);
        let mut summary = if shown.is_empty() {
            "No bytes differ between these two moments.".to_string()
        } else {
            format!(
                "{} byte{} in {} identifier{} differ.",
                shown.len(),
                if shown.len() == 1 { "" } else { "s" },
                compare::identifiers(&shown),
                if compare::identifiers(&shown) == 1 { "" } else { "s" }
            )
        };
        if left_out > 0 {
            summary.push_str(&format!(" {left_out} counter or clock byte{} left out.", if left_out == 1 { "" } else { "s" }));
        }
        ui.label(RichText::new(summary).color(INK));
        if a == b {
            ui.label(RichText::new("Both ends are the same marker.").size(12.0).color(WARN));
        }
        if shown.is_empty() {
            return;
        }
        ui.add_space(6.0);
        let phone = self.phone();
        egui::Grid::new("compare-table").num_columns(if phone { 5 } else { 6 }).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
            let headings: &[&str] = if phone {
                &["ID", "Byte", "From", "To", "Bits"]
            } else {
                &["ID", "Meaning", "Byte", "From", "To", "Bits"]
            };
            for heading in headings {
                ui.label(RichText::new(*heading).color(MUTED));
            }
            ui.end_row();
            for diff in &shown {
                ui.label(RichText::new(format!("0x{:03X}", diff.id)).monospace().color(INK));
                if !phone {
                    let name = frames::label(diff.id);
                    ui.label(RichText::new(if name.is_empty() { "Not decoded" } else { name }).color(MUTED));
                }
                ui.label(RichText::new(diff.byte.to_string()).monospace().color(INK));
                for side in [diff.a, diff.b] {
                    ui.label(RichText::new(side.map_or_else(|| "—".to_string(), |byte| format!("{byte:02X}"))).monospace().color(INK));
                }
                ui.label(RichText::new(diff.bits().map_or_else(String::new, bit_list)).color(WARN));
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        control_row(ui, phone, |ui| {
            if action_button(ui, "Save as CSV", true).clicked() {
                let table = compare_table(&shown, &name_of(a), &name_of(b));
                self.save_table("comparison", &table);
            }
            self.show_export_note(ui);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(id: &str) -> SignalDraft {
        SignalDraft {
            name: "Horn".into(),
            id: id.into(),
            ..SignalDraft::default()
        }
    }

    #[test]
    fn identifiers_are_read_as_hex_with_or_without_the_prefix() {
        assert_eq!(parse_id("182"), Some(0x182));
        assert_eq!(parse_id("0x182"), Some(0x182));
        assert_eq!(parse_id(" 0X7ff "), Some(0x7FF));
        assert_eq!(parse_id("7FF"), Some(0x7FF));
        assert_eq!(parse_id("800"), None);
        assert_eq!(parse_id(""), None);
        assert_eq!(parse_id("0x"), None);
        assert_eq!(parse_id("xyz"), None);
        assert_eq!(parse_id("12345"), None);
        assert_eq!(parse_id("-1"), None);
    }

    #[test]
    fn a_draft_becomes_a_signal_or_says_what_is_wrong() {
        let def = draft("0x182").to_def(false).unwrap();
        assert_eq!((def.id, def.start, def.count, def.scale, def.offset), (0x182, 0, 1, 1.0, 0.0));
        assert!(draft("").to_def(false).unwrap_err().contains("hex"));
        let mut bad = draft("182");
        bad.scale = "abc".into();
        assert!(bad.to_def(false).unwrap_err().contains("scale"));
        bad.scale = "0,5".into();
        assert_eq!(bad.to_def(false).unwrap().scale, 0.5, "a comma works as the decimal mark");
        bad.offset = "-40".into();
        assert_eq!(bad.to_def(false).unwrap().offset, -40.0);
        bad.offset = "inf".into();
        assert!(bad.to_def(false).is_err());
        bad.offset = "0".into();
        bad.scale = "0".into();
        assert!(bad.to_def(false).is_err());
        bad.scale = "1".into();
        bad.start = 6;
        bad.count = 4;
        assert!(bad.to_def(false).unwrap_err().contains("past the end"));
    }

    #[test]
    fn an_unnamed_draft_can_be_watched_but_not_saved() {
        let mut unnamed = draft("182");
        unnamed.name = "  ".into();
        assert!(unnamed.to_def(false).is_err());
        assert_eq!(unnamed.to_def(true).unwrap().name, "Draft");
    }

    #[test]
    fn the_bytes_are_kept_inside_the_frame() {
        let mut odd = draft("182");
        odd.start = 7;
        odd.count = 4;
        odd.fit();
        assert_eq!((odd.start, odd.count), (7, 1));
        odd.start = 200;
        odd.count = 0;
        odd.fit();
        assert_eq!((odd.start, odd.count), (7, 1));
        odd.start = 2;
        odd.count = 9;
        odd.fit();
        assert_eq!((odd.start, odd.count), (2, 4));
        assert!(odd.to_def(false).is_ok());
    }

    #[test]
    fn readings_are_shown_short() {
        assert_eq!(number_text(12.0), "12");
        assert_eq!(number_text(12.5), "12.5");
        assert_eq!(number_text(0.1 + 0.2), "0.3");
        assert_eq!(number_text(-0.0), "0");
        assert_eq!(number_text(-0.0001), "0");
        assert_eq!(number_text(1234.56), "1235");
        assert_eq!(number_text(123.456), "123.5");
        assert_eq!(number_text(f32::NAN), "—");
        assert_eq!(number_text(f32::INFINITY), "—");
        assert_eq!(number_text(-7.25), "-7.25");
    }

    #[test]
    fn bits_are_named() {
        assert_eq!(bit_list(0), "");
        assert_eq!(bit_list(0b0000_0100), "bit 2");
        assert_eq!(bit_list(0b1000_1001), "bits 0, 3, 7");
    }

    #[test]
    fn a_comparison_saves_as_a_table() {
        let diffs = vec![
            Diff { id: 0x280, byte: 2, a: Some(0x00), b: Some(0x04) },
            Diff { id: 0x7E0, byte: 3, a: None, b: Some(0x09) },
        ];
        let table = compare_table(&diffs, "#1 horn off", "#2 horn on");
        assert_eq!(table.headers[3], "#1 horn off");
        assert_eq!(table.rows[0], ["0x280", "Drive state", "2", "00", "04", "bit 2"]);
        assert_eq!(table.rows[1], ["0x7E0", "Not decoded", "3", "", "09", ""]);
    }

    #[test]
    fn a_plot_holds_between_readings_and_fills_its_box() {
        let points: VecDeque<(u64, f32)> = [(0u64, 0.0f32), (1_000, 10.0), (2_000, 10.0)].into_iter().collect();
        let (line, low, high) = plot_shape(&points, 2_000).unwrap();
        assert_eq!((low, high), (0.0, 10.0));
        // across, then up: each new reading is preceded by the old height.
        assert_eq!(line, [(0.0, 0.0), (0.5, 0.0), (0.5, 1.0), (1.0, 1.0), (1.0, 1.0)]);
        assert!(line.iter().all(|(x, y)| (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y)));
    }

    #[test]
    fn a_flat_plot_sits_in_the_middle_and_an_empty_one_has_no_shape() {
        let flat: VecDeque<(u64, f32)> = [(0u64, 5.0f32), (500, 5.0)].into_iter().collect();
        let (line, ..) = plot_shape(&flat, 1_000).unwrap();
        assert!(line.iter().all(|(_, y)| (*y - 0.5).abs() < 1e-6));
        assert!(plot_shape(&VecDeque::new(), 1_000).is_none());
    }

    #[test]
    fn a_plot_shows_only_its_window() {
        let points: VecDeque<(u64, f32)> = [(0u64, 100.0f32), (9_000, 1.0), (10_000, 2.0)].into_iter().collect();
        let (_, low, high) = plot_shape(&points, 2_000).unwrap();
        assert_eq!((low, high), (1.0, 2.0), "the old reading is out of the window");
    }
}
