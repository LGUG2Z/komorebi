//! The timeline: the whole recording as lanes of what happened, with the playhead on it.
//!
//! [`Open::timeline_panel`] draws the bottom panel: a bar of playing controls, and under it the
//! lanes. The arithmetic (which stretch is visible, where a time falls, where the ruler's marks go,
//! how to bin a dense lane) is in [`timeline`](super::timeline) and is tested there; this file
//! draws it and turns the mouse into actions. The lanes, and what the mouse does on them, are
//! described on [`Open::timeline_panel`], whose documentation is public.
//!
//! It needs an `egui::Context`, so there are no examples; the tests in `viewer::app` draw it with
//! real input.

use super::app::Open;
use super::app::Tab;
use super::theme;
use super::timeline;
use super::timeline::Bin;
use crate::timecode;
use eframe::egui;
use eframe::egui::Color32;
use eframe::egui::Pos2;
use eframe::egui::Rect;
use eframe::egui::Stroke;
use eframe::egui::vec2;

const LABEL_WIDTH: f32 = 84.0;
const RULER: f32 = 22.0;
const BUGS: f32 = 22.0;
const EVENTS: f32 = 38.0;
const LOG: f32 = 16.0;
const MACHINE: f32 = 16.0;
const FINDINGS: f32 = 16.0;
const SCREEN: f32 = 16.0;
const VIDEO: f32 = 12.0;
const GAP: f32 = 2.0;

/// More events than this in view are drawn as columns rather than one by one.
const INDIVIDUAL_EVENTS: usize = 1500;

/// A drag that is going on across the lanes.
///
/// A press picks which it is, and the drag carries on as that until the pointer is released:
/// a press near an edge of a bug region in the bugs lane takes hold of the edge, else a press
/// with `Shift` held picks a range, else it scrubs.
#[derive(Debug, Clone, PartialEq)]
pub enum Drag {
    /// Moving the playhead.
    Scrub,
    /// Picking a range, from where the drag began.
    Select {
        /// Where the drag began, in session video seconds. The selection runs between there and the
        /// pointer, in time order.
        from: f64,
    },
    /// Moving the start or the end of a bug region.
    Edge {
        /// The id of the bug region.
        id: String,
        /// `true` to move the start of the region, `false` to move its end.
        moving_start: bool,
    },
}

/// How close, in pixels, a press must be to the edge of a bug region to take hold of it.
const GRAB_PX: f32 = 8.0;

/// Which lane a point is in. The names of the lanes are in the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    Ruler,
    Bugs,
    Events,
    Log,
    Machine,
    Findings,
    /// When the picture changed.
    Screen,
    Video(usize),
}

/// Where each lane sits down the panel: the lane, its top and its height, in points from the top
/// of the lanes. Lanes are stacked in the order of [`Lane`], with [`GAP`] between them, and one
/// `Video` lane for each display after the others.
struct Layout {
    lanes: Vec<(Lane, f32, f32)>,
}

impl Layout {
    fn new(displays: usize) -> Self {
        let mut heights = vec![
            (Lane::Ruler, RULER),
            (Lane::Bugs, BUGS),
            (Lane::Events, EVENTS),
            (Lane::Log, LOG),
            (Lane::Machine, MACHINE),
            (Lane::Findings, FINDINGS),
            (Lane::Screen, SCREEN),
        ];
        heights.extend((0..displays).map(|index| (Lane::Video(index), VIDEO)));

        let mut top = 0.0;
        Self {
            lanes: heights
                .into_iter()
                .map(|(lane, height)| {
                    let entry = (lane, top, height);
                    top += height + GAP;
                    entry
                })
                .collect(),
        }
    }

    fn height(&self) -> f32 {
        self.lanes
            .last()
            .map_or(0.0, |(_, top, height)| top + height)
    }

    fn at(&self, y: f32) -> Option<Lane> {
        self.lanes
            .iter()
            .find(|(_, top, height)| y >= *top && y <= top + height + GAP)
            .map(|(lane, _, _)| *lane)
    }

    fn rect(&self, lane: Lane, area: Rect) -> Rect {
        let (_, top, height) = self
            .lanes
            .iter()
            .find(|(candidate, _, _)| *candidate == lane)
            .copied()
            .unwrap_or((lane, 0.0, 0.0));
        Rect::from_min_size(
            Pos2::new(area.left(), area.top() + top),
            vec2(area.width(), height),
        )
    }
}

impl Open {
    /// Draws the bottom panel: the bar of playing controls, and under it the timeline.
    ///
    /// The bar has the buttons for the transport, the playhead's time and the length of the
    /// recording, a menu of speeds (0.25, 0.5, 1, 2, 4 and 8 times; choosing one keeps the
    /// direction), a "loop" toggle that repeats the selection, the selection if there is one, and
    /// the "fit" and "Mark bug" buttons. Each acts by [`Open::apply`] with the
    /// [`Action`](super::keymap::Action) of the key that does the same.
    ///
    /// Needs an `egui::Context`, so a window is needed. The panel has a fixed height: that of the
    /// lanes plus 46 points for the bar.
    ///
    /// # The lanes
    ///
    /// From the top, each lane is 12 to 38 points tall with 2 points between them. The names of the
    /// lanes are at the left, and one lane is added at the bottom for each display that has video.
    ///
    /// | lane | what is drawn | a click |
    /// |---|---|---|
    /// | ruler | marks and times ([`ruler`](super::timeline::ruler), labels at least 70 points apart) | seeks |
    /// | bugs | the bug regions the user marked, labeled with the title or the first line of the text; an open-ended one ends in "…" | selects the region under it, shows the Bugs tab and seeks to its start; elsewhere seeks |
    /// | events | komorebi's events, one mark each, colored by class and taller the louder the class | selects the event within 6 points, else seeks |
    /// | log | the errors and warnings of komorebi's log, as columns (red for an error) | seeks |
    /// | machine | incidents: a display going away, sleep, lock (a diamond each) | seeks to the incident within 6 points, else to the click |
    /// | findings | what looks wrong, as bars colored by severity | selects the finding under it, shows the Findings tab and seeks to its start; elsewhere seeks |
    /// | screen | where the picture changed, brighter the more of the screen did; "looking for changes in the picture…" until the scan has run | seeks |
    /// | video *N* | the stretches with video from display *N* | seeks |
    ///
    /// Over the lanes under the ruler the bug regions are tinted faintly and the selection is
    /// shaded, and the playhead is a line with a handle at the top, over all of them. When more
    /// than 1500 events are in view they are drawn as columns that count them (the height is the
    /// logarithm of the count, the color the loudest class in the column) instead of one by one.
    ///
    /// # The mouse
    ///
    /// | gesture | effect |
    /// |---|---|
    /// | click | as in the table above |
    /// | drag | scrubs: the playhead follows the pointer |
    /// | `Shift` and drag | picks a range, which becomes the selection |
    /// | drag from within 8 points of an edge of a bug region in the bugs lane | moves that edge (an open-ended region has no end to take hold of); the region is saved when the drag ends |
    /// | pinch, or `Command` and scroll | zooms around the pointer |
    /// | scroll | pans |
    /// | hover | a tooltip about what is under the pointer (an event, an incident, a finding, a bug region, a change in the picture, an error or warning in the log); elsewhere, the time |
    ///
    /// Dragging an edge saves through the same append-only file the command line reads, and a region
    /// cannot be made to end before it starts (a refusal is shown in the top bar).
    ///
    /// The arithmetic is in [`timeline`](super::timeline) and is tested there.
    pub fn timeline_panel(&mut self, ctx: &egui::Context) {
        let layout = Layout::new(self.loaded.displays.len());
        egui::TopBottomPanel::bottom("timeline")
            .frame(
                egui::Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .exact_height(layout.height() + 46.0)
            .show(ctx, |ui| {
                self.transport_ui(ui);
                ui.add_space(4.0);
                self.lanes_ui(ui, &layout);
            });
    }

    fn transport_ui(&mut self, ui: &mut egui::Ui) {
        let playing = self.playback.is_playing();
        let rate = self.playback.rate();
        let position = self.position();

        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Start, "to the start (Home)") {
                self.apply(super::keymap::Action::ToStart);
            }
            if icon_button(ui, Icon::StepBack, "back one frame (←)") {
                self.apply(super::keymap::Action::StepFrame(-1));
            }
            if icon_button(ui, Icon::Reverse, "play backwards (J)") {
                self.apply(super::keymap::Action::PlayBackwards);
            }
            if icon_button(
                ui,
                if playing { Icon::Pause } else { Icon::Play },
                "play / pause (Space)",
            ) {
                self.apply(super::keymap::Action::TogglePlay);
            }
            if icon_button(ui, Icon::Forward, "play forwards (L)") {
                self.apply(super::keymap::Action::PlayForwards);
            }
            if icon_button(ui, Icon::StepForward, "forward one frame (→)") {
                self.apply(super::keymap::Action::StepFrame(1));
            }
            if icon_button(ui, Icon::End, "to the end (End)") {
                self.apply(super::keymap::Action::ToEnd);
            }

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} / {}",
                    timecode::format(position),
                    timecode::format(self.loaded.duration_s)
                ))
                .monospace()
                .size(15.0),
            );

            ui.add_space(8.0);
            let mut magnitude = rate.abs();
            egui::ComboBox::from_id_salt("rate")
                .width(64.0)
                .selected_text(format!("{}{magnitude}×", if rate < 0.0 { "−" } else { "" }))
                .show_ui(ui, |ui| {
                    for choice in [0.25, 0.5, 1.0, 2.0, 4.0, 8.0] {
                        ui.selectable_value(&mut magnitude, choice, format!("{choice}×"));
                    }
                });
            if (magnitude - rate.abs()).abs() > 1e-9 {
                self.playback
                    .set_rate(magnitude * if rate < 0.0 { -1.0 } else { 1.0 });
            }

            ui.add_space(8.0);
            let looping = self.playback.looping().is_some();
            if ui
                .selectable_label(looping, "loop")
                .on_hover_text("repeat the selection (\\)")
                .clicked()
            {
                self.apply(super::keymap::Action::ToggleLoop);
            }

            if let Some((from, to)) = self.selection {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "selected {} – {} ({:.2} s)",
                        timecode::format(from),
                        timecode::format(to),
                        to - from
                    ))
                    .color(theme::SELECTION),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("Mark bug").color(Color32::WHITE))
                            .fill(Color32::from_rgb(150, 40, 40)),
                    )
                    .on_hover_text("mark where the bug starts and ends (B)")
                    .clicked()
                {
                    self.apply(super::keymap::Action::MarkBug);
                }
                if ui
                    .small_button("fit")
                    .on_hover_text("show the whole recording (0)")
                    .clicked()
                {
                    self.apply(super::keymap::Action::FitTimeline);
                }
            });
        });
    }

    fn lanes_ui(&mut self, ui: &mut egui::Ui, layout: &Layout) {
        let (area, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), layout.height()),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(area);
        let time_area = Rect::from_min_max(
            Pos2::new(area.left() + LABEL_WIDTH, area.top()),
            area.right_bottom(),
        );
        let (left, width) = (time_area.left(), time_area.width());
        let view = self.view;
        let duration = self.loaded.duration_s;
        let position = self.position();

        // stripes and their names
        for (index, (lane, _, _)) in layout.lanes.iter().enumerate() {
            let strip = layout.rect(*lane, area);
            if *lane != Lane::Ruler {
                painter.rect_filled(
                    strip,
                    2.0,
                    if index % 2 == 0 {
                        theme::LANE
                    } else {
                        theme::LANE_ALT
                    },
                );
            }
            let name = match lane {
                Lane::Ruler => String::new(),
                Lane::Bugs => "bugs".to_string(),
                Lane::Events => "events".to_string(),
                Lane::Log => "log".to_string(),
                Lane::Machine => "machine".to_string(),
                Lane::Findings => "findings".to_string(),
                Lane::Screen => "screen".to_string(),
                Lane::Video(display) => format!("video {}", self.loaded.displays[*display].id),
            };
            painter.text(
                Pos2::new(strip.left() + 4.0, strip.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(10.5),
                theme::TEXT_DIM,
            );
        }

        // the ruler
        let ruler = layout.rect(Lane::Ruler, area);
        for tick in timeline::ruler(&view, width, 70.0) {
            let x = view.x(tick.time, left, width);
            painter.line_segment(
                [
                    Pos2::new(x, ruler.bottom() - if tick.major { 9.0 } else { 4.0 }),
                    Pos2::new(x, ruler.bottom()),
                ],
                Stroke::new(1.0_f32, theme::TEXT_DIM),
            );
            if tick.major {
                painter.text(
                    Pos2::new(x + 3.0, ruler.top() + 1.0),
                    egui::Align2::LEFT_TOP,
                    tick.label,
                    egui::FontId::monospace(10.0),
                    theme::TEXT_DIM,
                );
            }
        }

        // video coverage
        for (index, display) in self.loaded.displays.iter().enumerate() {
            let strip = layout.rect(Lane::Video(index), area);
            for (from, to) in &display.segments {
                let (x0, x1) = (
                    view.x(*from, left, width).max(left),
                    view.x(*to, left, width).min(area.right()),
                );
                if x1 > x0 {
                    painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(x0, strip.top() + 2.0),
                            Pos2::new(x1, strip.bottom() - 2.0),
                        ),
                        1.0,
                        theme::VIDEO,
                    );
                }
            }
        }

        // what the user marked as the bug, over its own lane and faintly over the rest
        let bugs_strip = layout.rect(Lane::Bugs, area);
        for bug in self.loaded.bugs() {
            // while an edge is being dragged, show where it would land
            let (start_s, end_s) = match &self.edge_preview {
                Some((id, start, end)) if *id == bug.id => (*start, *end),
                _ => (bug.start_video_s, bug.end_video_s),
            };
            let (x0, x1) = (
                view.x(start_s, left, width).clamp(left, area.right()),
                view.x(end_s, left, width).clamp(left, area.right()),
            );
            if x1 < left || x0 > area.right() {
                continue;
            }
            let selected = self.selected_bug.as_deref() == Some(bug.id.as_str());
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x0, bugs_strip.top() + 1.0),
                    Pos2::new(x1.max(x0 + 2.0), bugs_strip.bottom() - 1.0),
                ),
                2.0,
                Color32::from_rgba_unmultiplied(230, 70, 60, if selected { 200 } else { 130 }),
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x0, area.top() + RULER),
                    Pos2::new(x1.max(x0 + 2.0), area.bottom()),
                ),
                0.0,
                Color32::from_rgba_unmultiplied(230, 70, 60, 22),
            );
            let label = if bug.title.is_empty() {
                bug.text.lines().next().unwrap_or("").to_string()
            } else {
                bug.title.clone()
            };
            if x1 - x0 > 30.0 {
                painter
                    .with_clip_rect(Rect::from_min_max(
                        Pos2::new(x0, bugs_strip.top()),
                        Pos2::new(x1, bugs_strip.bottom()),
                    ))
                    .text(
                        Pos2::new(x0 + 4.0, bugs_strip.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(11.0),
                        Color32::WHITE,
                    );
            }
            if bug.open_ended {
                painter.text(
                    Pos2::new(x1 - 2.0, bugs_strip.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "…",
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
            }
        }

        // the range being picked
        if let Some((from, to)) = self.selection {
            let (x0, x1) = (view.x(from, left, width), view.x(to, left, width));
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x0.max(left), area.top() + RULER),
                    Pos2::new(x1.min(area.right()), area.bottom()),
                ),
                0.0,
                Color32::from_rgba_unmultiplied(90, 150, 240, 45),
            );
            for x in [x0, x1] {
                painter.line_segment(
                    [
                        Pos2::new(x, area.top() + RULER),
                        Pos2::new(x, area.bottom()),
                    ],
                    Stroke::new(1.0_f32, theme::SELECTION),
                );
            }
        }

        // events: one mark each while they are few enough to tell apart, columns when not
        let events_strip = layout.rect(Lane::Events, area);
        let visible = self
            .loaded
            .event_times
            .iter()
            .filter(|time| **time >= view.start_s && **time <= view.end_s)
            .count();
        if visible <= INDIVIDUAL_EVENTS {
            for event in &self.loaded.events {
                if event.video_s < view.start_s || event.video_s > view.end_s {
                    continue;
                }
                let x = view.x(event.video_s, left, width);
                let weight = theme::class_weight(event.class);
                let reach = 8.0 + f32::from(weight) * 3.0;
                let selected = self.selected_event == Some(event.seq);
                painter.line_segment(
                    [
                        Pos2::new(x, events_strip.center().y - reach / 2.0),
                        Pos2::new(x, events_strip.center().y + reach / 2.0),
                    ],
                    Stroke::new(
                        if selected { 3.0 } else { 1.5_f32 },
                        if selected {
                            Color32::WHITE
                        } else {
                            theme::class(event.class)
                        },
                    ),
                );
            }
        } else {
            let columns = timeline::bin(
                self.loaded
                    .events
                    .iter()
                    .map(|event| (event.video_s, theme::class_weight(event.class))),
                &view,
                width as usize,
            );
            let peak = columns.iter().map(|bin| bin.count).max().unwrap_or(1) as f32;
            for Bin {
                column,
                count,
                weight,
            } in columns
            {
                let x = left + column as f32 + 0.5;
                let height = (events_strip.height() - 4.0)
                    * ((count as f32).ln_1p() / peak.ln_1p()).clamp(0.12, 1.0);
                painter.line_segment(
                    [
                        Pos2::new(x, events_strip.bottom() - 2.0),
                        Pos2::new(x, events_strip.bottom() - 2.0 - height),
                    ],
                    Stroke::new(1.0_f32, class_colour_of_weight(weight)),
                );
            }
        }

        // errors and warnings in komorebi's log
        let log_strip = layout.rect(Lane::Log, area);
        let marks = timeline::bin(
            self.loaded.log_marks.iter().map(|mark| {
                (
                    mark.video_s,
                    u8::from(mark.level == crate::analysis::logparse::Level::Error) + 1,
                )
            }),
            &view,
            width as usize,
        );
        for Bin {
            column,
            count,
            weight,
        } in marks
        {
            let x = left + column as f32 + 0.5;
            let height = (2.0 + (count as f32).ln_1p() * 3.0).min(log_strip.height() - 2.0);
            painter.line_segment(
                [
                    Pos2::new(x, log_strip.bottom() - 1.0),
                    Pos2::new(x, log_strip.bottom() - 1.0 - height),
                ],
                Stroke::new(
                    1.0_f32,
                    if weight > 1 {
                        theme::level(crate::analysis::logparse::Level::Error)
                    } else {
                        theme::level(crate::analysis::logparse::Level::Warn)
                    },
                ),
            );
        }

        // what happened to the machine
        let machine_strip = layout.rect(Lane::Machine, area);
        for incident in &self.loaded.incidents {
            let x = view.x(incident.video_s, left, width);
            if x < left || x > area.right() {
                continue;
            }
            let centre = Pos2::new(x, machine_strip.center().y);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    centre + vec2(0.0, -5.0),
                    centre + vec2(5.0, 0.0),
                    centre + vec2(0.0, 5.0),
                    centre + vec2(-5.0, 0.0),
                ],
                theme::INCIDENT,
                Stroke::NONE,
            ));
        }

        // what looked wrong
        let findings_strip = layout.rect(Lane::Findings, area);
        for (index, finding) in self.loaded.findings.iter().enumerate() {
            if finding.end_video_s < view.start_s || finding.start_video_s > view.end_s {
                continue;
            }
            let (x0, x1) = (
                view.x(finding.start_video_s, left, width).max(left),
                view.x(finding.end_video_s, left, width).min(area.right()),
            );
            let colour = theme::severity(finding.severity);
            let selected = self.selected_finding == Some(index);
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(
                        x0,
                        findings_strip.center().y - if selected { 4.0 } else { 2.5 },
                    ),
                    Pos2::new(
                        x1.max(x0 + 3.0),
                        findings_strip.center().y + if selected { 4.0 } else { 2.5 },
                    ),
                ),
                1.0,
                colour,
            );
        }

        // when the picture changed, once the scan has found it
        let screen_strip = layout.rect(Lane::Screen, area);
        if let Some(changes) = &self.changes {
            for span in &changes.spans {
                let (x0, x1) = (
                    view.x(span.start_video_s, left, width).max(left),
                    view.x(span.end_video_s, left, width).min(area.right()),
                );
                if x1 < left || x0 > area.right() {
                    continue;
                }
                let strength = (span.peak_share * 40.0).clamp(0.25, 1.0) as f32;
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(x0, screen_strip.top() + 3.0),
                        Pos2::new(x1.max(x0 + 2.0), screen_strip.bottom() - 3.0),
                    ),
                    1.0,
                    Color32::from_rgba_unmultiplied(120, 200, 190, (90.0 + 150.0 * strength) as u8),
                );
            }
        } else if self.changes_result.is_some() {
            painter.text(
                Pos2::new(left + 6.0, screen_strip.center().y),
                egui::Align2::LEFT_CENTER,
                "looking for changes in the picture…",
                egui::FontId::proportional(10.0),
                theme::TEXT_DIM,
            );
        }

        // the playhead
        let x = view.x(position, left, width);
        if x >= left - 1.0 && x <= area.right() + 1.0 {
            painter.line_segment(
                [Pos2::new(x, area.top()), Pos2::new(x, area.bottom())],
                Stroke::new(1.5_f32, theme::PLAYHEAD),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x - 5.0, area.top()),
                    Pos2::new(x + 5.0, area.top()),
                    Pos2::new(x, area.top() + 8.0),
                ],
                theme::PLAYHEAD,
                Stroke::NONE,
            ));
        }

        self.lanes_interaction(ui, &response, layout, area, time_area, duration);
    }

    fn lanes_interaction(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        layout: &Layout,
        area: Rect,
        time_area: Rect,
        duration: f64,
    ) {
        let (left, width) = (time_area.left(), time_area.width());
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| response.hover_pos());
        let shift = ui.input(|input| input.modifiers.shift);

        // zoom with a pinch or with Command-scroll, pan with scrolling
        if response.hovered()
            && let Some(hover) = response.hover_pos()
        {
            let (scroll, zoom, command) = ui.input(|input| {
                (
                    input.smooth_scroll_delta,
                    input.zoom_delta(),
                    input.modifiers.command,
                )
            });
            let anchor = self.view.time(hover.x, left, width);
            if (zoom - 1.0).abs() > 1e-4 {
                self.view.zoom(anchor, f64::from(zoom), duration);
            } else if command && scroll.y.abs() > 0.0 {
                self.view.zoom(
                    anchor,
                    (1.0 + f64::from(scroll.y) * 0.01).max(0.1),
                    duration,
                );
            } else if scroll.x.abs() > 0.0 || scroll.y.abs() > 0.0 {
                let pixels = if scroll.x.abs() > scroll.y.abs() {
                    scroll.x
                } else {
                    scroll.y
                };
                self.view.pan(
                    -f64::from(pixels) / f64::from(width) * self.view.span(),
                    duration,
                );
            }
        }

        // a press picks scrubbing, selecting or taking hold of the edge of a bug region; the drag
        // then carries on
        if response.drag_started()
            && let Some(point) = pointer
            && point.x >= left
        {
            let time = self.view.time(point.x, left, width).clamp(0.0, duration);
            let reach = f64::from(GRAB_PX / width) * self.view.span();
            let in_bugs_lane = layout.at(point.y - area.top()) == Some(Lane::Bugs);

            let edge = in_bugs_lane
                .then(|| {
                    self.loaded.bugs().find_map(|bug| {
                        if (bug.start_video_s - time).abs() <= reach {
                            Some((bug.id.clone(), true, bug.start_video_s, bug.end_video_s))
                        } else if !bug.open_ended && (bug.end_video_s - time).abs() <= reach {
                            Some((bug.id.clone(), false, bug.start_video_s, bug.end_video_s))
                        } else {
                            None
                        }
                    })
                })
                .flatten();

            self.drag = Some(if let Some((id, moving_start, start, end)) = edge {
                self.edge_preview = Some((id.clone(), start, end));
                Drag::Edge { id, moving_start }
            } else if shift {
                Drag::Select { from: time }
            } else {
                Drag::Scrub
            });
        }

        if response.dragged()
            && let Some(point) = pointer
        {
            let time = self.view.time(point.x, left, width).clamp(0.0, duration);
            match self.drag.clone() {
                Some(Drag::Scrub) => {
                    self.playback.seek(time);
                    self.view.reveal(time, duration);
                }
                Some(Drag::Select { from }) => {
                    self.selection = Some((from.min(time), from.max(time)));
                }
                Some(Drag::Edge { id, moving_start }) => {
                    if let Some((_, start, end)) = &mut self.edge_preview {
                        if moving_start {
                            *start = time.min(*end);
                        } else {
                            *end = time.max(*start);
                        }
                    }
                    let _ = id;
                }
                None => {}
            }
        }
        if response.drag_stopped() {
            if let Some(Drag::Edge { id, moving_start }) = self.drag.take()
                && let Some((_, start, end)) = self.edge_preview.take()
            {
                let result =
                    self.move_bug_edge(&id, moving_start, if moving_start { start } else { end });
                match result {
                    Ok(()) => self.say("region changed"),
                    Err(message) => self.say(message),
                }
            }
            self.edge_preview = None;
            self.drag = None;
        }

        // a click goes to what is under it, else to the time
        if response.clicked()
            && let Some(point) = response.interact_pointer_pos()
            && point.x >= left
        {
            let time = self.view.time(point.x, left, width).clamp(0.0, duration);
            let reach = f64::from(6.0 / width) * self.view.span();
            match layout.at(point.y - area.top()) {
                Some(Lane::Events) => {
                    if let Some(index) = timeline::nearest(&self.loaded.event_times, time, reach) {
                        self.select_event(index);
                    } else {
                        self.playback.seek(time);
                    }
                }
                Some(Lane::Bugs) => {
                    let hit = self
                        .loaded
                        .bugs()
                        .find(|bug| {
                            time >= bug.start_video_s - reach && time <= bug.end_video_s + reach
                        })
                        .map(|bug| (bug.id.clone(), bug.start_video_s));
                    match hit {
                        Some((id, start)) => {
                            self.selected_bug = Some(id);
                            self.tab = Tab::Bugs;
                            self.playback.seek(start);
                        }
                        None => self.playback.seek(time),
                    }
                }
                Some(Lane::Findings) => {
                    let hit = self.loaded.findings.iter().position(|finding| {
                        time >= finding.start_video_s - reach && time <= finding.end_video_s + reach
                    });
                    if let Some(index) = hit {
                        self.selected_finding = Some(index);
                        self.tab = Tab::Findings;
                        self.playback
                            .seek(self.loaded.findings[index].start_video_s);
                    } else {
                        self.playback.seek(time);
                    }
                }
                Some(Lane::Machine) => {
                    let times: Vec<f64> = self
                        .loaded
                        .incidents
                        .iter()
                        .map(|incident| incident.video_s)
                        .collect();
                    let target =
                        timeline::nearest(&times, time, reach).map_or(time, |index| times[index]);
                    self.playback.seek(target);
                }
                _ => self.playback.seek(time),
            }
        }

        // what is under the pointer
        if let Some(hover) = response.hover_pos()
            && hover.x >= left
        {
            let time = self.view.time(hover.x, left, width);
            let reach = f64::from(5.0 / width) * self.view.span();
            let text = match layout.at(hover.y - area.top()) {
                Some(Lane::Events) => timeline::nearest(&self.loaded.event_times, time, reach)
                    .and_then(|index| self.loaded.events.get(index))
                    .map(|event| {
                        format!(
                            "#{} {}  ·  {}\n{}",
                            event.seq,
                            timecode::format(event.video_s),
                            format!("{:?}", event.class).to_lowercase(),
                            event.summary
                        )
                    }),
                Some(Lane::Machine) => self
                    .loaded
                    .incidents
                    .iter()
                    .find(|incident| (incident.video_s - time).abs() <= reach)
                    .map(|incident| {
                        format!(
                            "{}  ·  {}\n{}",
                            timecode::format(incident.video_s),
                            incident.kind,
                            incident.detail
                        )
                    }),
                Some(Lane::Findings) => self
                    .loaded
                    .findings
                    .iter()
                    .find(|finding| {
                        time >= finding.start_video_s - reach && time <= finding.end_video_s + reach
                    })
                    .map(|finding| {
                        format!(
                            "{}  ·  {}\n{}",
                            timecode::format(finding.start_video_s),
                            finding.kind,
                            finding.summary
                        )
                    }),
                Some(Lane::Bugs) => self
                    .loaded
                    .bugs()
                    .find(|bug| {
                        time >= bug.start_video_s - reach && time <= bug.end_video_s + reach
                    })
                    .map(|bug| {
                        format!(
                            "bug  {} – {}\n{}",
                            timecode::format(bug.start_video_s),
                            timecode::format(bug.end_video_s),
                            bug.text
                        )
                    }),
                Some(Lane::Screen) => self.changes.as_ref().and_then(|changes| {
                    changes
                        .spans
                        .iter()
                        .find(|span| time >= span.start_video_s - reach && time <= span.end_video_s + reach)
                        .map(|span| {
                            format!(
                                "display {}  ·  {} – {}\nthe picture changed here: up to {:.1}% of the screen",
                                span.display,
                                timecode::format(span.start_video_s),
                                timecode::format(span.end_video_s),
                                span.peak_share * 100.0
                            )
                        })
                }),
                Some(Lane::Log) => {
                    let times: Vec<f64> = self
                        .loaded
                        .log_marks
                        .iter()
                        .map(|mark| mark.video_s)
                        .collect();
                    timeline::nearest(&times, time, reach * 2.0).map(|index| {
                        format!(
                            "{}  ·  {:?} in komorebi's log",
                            timecode::format(times[index]),
                            self.loaded.log_marks[index].level
                        )
                    })
                }
                _ => Some(timecode::format(time)),
            };

            if let Some(text) = text {
                response.clone().on_hover_ui_at_pointer(|ui| {
                    ui.label(text);
                });
            }
        }
    }
}

fn class_colour_of_weight(weight: u8) -> Color32 {
    match weight {
        6 => Color32::from_rgb(205, 130, 240),
        5 => Color32::from_rgb(110, 180, 255),
        4 => Color32::from_rgb(120, 205, 145),
        3 => Color32::from_rgb(245, 170, 90),
        2 => Color32::from_rgb(170, 170, 200),
        _ => Color32::from_rgb(88, 88, 100),
    }
}

/// The pictures on the buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Icon {
    Start,
    StepBack,
    Reverse,
    Play,
    Pause,
    Forward,
    StepForward,
    End,
}

/// A button drawn with shapes, since the fonts may not have the symbols.
fn icon_button(ui: &mut egui::Ui, icon: Icon, tip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 24.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 4.0, Color32::from_gray(48));
    }

    let colour = if response.hovered() {
        Color32::WHITE
    } else {
        Color32::from_gray(200)
    };
    let centre = rect.center();
    let triangle = |direction: f32, offset: f32| {
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(centre.x + offset - 4.0 * direction, centre.y - 6.0),
                Pos2::new(centre.x + offset - 4.0 * direction, centre.y + 6.0),
                Pos2::new(centre.x + offset + 5.0 * direction, centre.y),
            ],
            colour,
            Stroke::NONE,
        ));
    };
    let bar = |offset: f32| {
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(centre.x + offset, centre.y), vec2(2.5, 12.0)),
            0.0,
            colour,
        );
    };

    match icon {
        Icon::Start => {
            bar(-6.0);
            triangle(-1.0, 3.0);
        }
        Icon::StepBack => {
            triangle(-1.0, -2.0);
            bar(6.0);
        }
        Icon::Reverse => triangle(-1.0, 0.0),
        Icon::Play => triangle(1.0, 0.0),
        Icon::Pause => {
            bar(-4.0);
            bar(4.0);
        }
        Icon::Forward => triangle(1.0, 0.0),
        Icon::StepForward => {
            bar(-6.0);
            triangle(1.0, 2.0);
        }
        Icon::End => {
            triangle(1.0, -3.0);
            bar(6.0);
        }
    }

    response.on_hover_text(tip).clicked()
}
