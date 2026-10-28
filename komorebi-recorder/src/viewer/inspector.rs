//! The panel on the right: what the thing under the playhead is, and the detail behind it.
//!
//! Five tabs ([`Tab`]):
//!
//! | tab | what it shows |
//! |---|---|
//! | *Now* | the state at the playhead and what has just happened |
//! | *Event* | one event end to end: what its handler logged, with the line of komorebi's source for each; what changed and who changed it; and what the rest of the system did around it (komorebi's own calls into the OS, the system, each application), which opens out into the records themselves |
//! | *Findings* | what looks wrong, ranked |
//! | *Bugs* | what was marked as the bug and what has been concluded |
//! | *Log* | komorebi's own log, following the playhead |
//!
//! Drawing the panel does not act on the session. What a click asks for (go to a time, select an
//! event, edit or delete a bug region, copy a command) is collected while drawing and carried out
//! once drawing is done, because drawing borrows the session and acting on it needs it mutably.
//!
//! An event is shown through [`Analysis::trace`](crate::analysis::Analysis::trace) and
//! [`Analysis::os_trace`](crate::analysis::Analysis::os_trace), worked out the first time the
//! Event tab shows the event and kept in [`Open::traces`] and [`Open::os_traces`]. The panel needs
//! an `egui::Context`, so there are no examples; `every_tab_of_the_inspector_draws` in `viewer::app`
//! draws each tab.

use super::app::Open;
use super::app::Tab;
use super::theme;
use crate::analysis::classify::Class;
use crate::analysis::ghost::GhostTile;
use crate::analysis::os_trace::GroupKind;
use crate::analysis::os_trace::OsTraceData;
use crate::analysis::os_trace::Phase;
use crate::analysis::trace::TraceData;
use crate::annotations::Kind;
use crate::asc::Platform;
use crate::asc::rule::Rule;
use crate::timecode;
use eframe::egui;
use eframe::egui::Color32;
use eframe::egui::RichText;
use std::sync::Arc;

/// What the user asked for in the panel, carried out once drawing is done (drawing borrows the
/// session, acting on it needs it mutably).
#[derive(Default)]
struct Wants {
    seek: Option<f64>,
    select_event: Option<u64>,
    select_finding: Option<usize>,
    select_bug: Option<String>,
    edit_bug: Option<String>,
    delete_bug: Option<String>,
    loop_region: Option<(f64, f64)>,
    copy: Option<String>,
    mark: bool,
    propose_rule: Option<u64>,
}

fn chip(ui: &mut egui::Ui, text: &str, colour: Color32) {
    ui.label(
        RichText::new(format!(" {text} "))
            .small()
            .color(Color32::BLACK)
            .background_color(colour),
    );
}

/// What the Bugs tab says under a bug region that was marked while a ghost tile was there: that
/// it looks like one, and the button that proposes a rule for it.
fn ghost_name(ghost: &GhostTile) -> String {
    match ghost.platform {
        Platform::Windows => format!("{} ({})", ghost.exe, ghost.class),
        Platform::Macos if ghost.title.is_empty() => ghost.exe.clone(),
        Platform::Macos => format!("{} ({:?})", ghost.exe, ghost.title),
    }
}

fn ghost_card(ui: &mut egui::Ui, ghost: &GhostTile, wants: &mut Wants) {
    egui::Frame::new()
        .fill(Color32::from_rgb(40, 38, 24))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                chip(ui, "ghost tile", Color32::from_rgb(235, 185, 60));
                ui.label(RichText::new(ghost_name(ghost)).strong());
            });
            ui.label(
                RichText::new(format!(
                    "komorebi tiled it at {}, and on {} reads from {} it was {}, and never on \
                     screen: an empty tile, where a person expects a window.",
                    ghost.place,
                    ghost.reads,
                    timecode::format(ghost.first_video_s),
                    ghost.hidden_how,
                ))
                .small()
                .color(theme::TEXT_DIM),
            );
            let rule = Rule::for_ghost(ghost);
            if rule.is_ok() {
                if ui
                    .button("Propose a rule to komorebi-application-specific-configuration…")
                    .on_hover_text("shows the rule first; nothing is sent until you say so")
                    .clicked()
                {
                    wants.propose_rule = Some(ghost.hwnd);
                }
            } else {
                let why = rule
                    .err()
                    .map_or_else(String::new, |error| error.to_string());
                ui.label(
                    RichText::new(format!(
                        "No rule can be written that catches it and nothing else: {why}."
                    ))
                    .small()
                    .color(theme::TEXT_DIM),
                );
            }
        });
}

fn class_name(class: Class) -> String {
    format!("{class:?}").to_lowercase()
}

fn actor_colour(actor: &str) -> Color32 {
    match actor {
        "handler" => theme::ACCENT,
        "reaper" => Color32::from_rgb(235, 185, 60),
        "workspace-reconciliator" => Color32::from_rgb(205, 130, 240),
        _ => theme::BUG,
    }
}

impl Open {
    /// Draws the panel on the right, with the tab chosen in [`Open::tab`] showing, and then carries
    /// out what the user asked for in it.
    ///
    /// The tabs act on the session through the same methods as the keys and the timeline: going
    /// somewhere pauses and moves the playhead and scrolls the timeline to it, and selecting an
    /// event shows the Event tab. The buttons to mark and edit a bug region open the composer,
    /// delete calls [`Open::delete_annotation`], and "loop it" repeats the region. Other buttons
    /// copy text: a `trace` command for the event, a command for the records of a source, the
    /// location of a line of komorebi's source, and a ready-to-paste prompt that starts an
    /// agent's investigation (see `agent_prompt` in the `inspector` module).
    ///
    /// Needs an `egui::Context`, so a window is needed.
    pub fn inspector_panel(&mut self, ctx: &egui::Context) {
        let mut wants = Wants::default();

        egui::SidePanel::right("inspector")
            .default_width(430.0)
            .min_width(300.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, name) in [
                        (Tab::Now, "Now"),
                        (Tab::Event, "Event"),
                        (Tab::Findings, "Findings"),
                        (Tab::Bugs, "Bugs"),
                        (Tab::Log, "Log"),
                    ] {
                        ui.selectable_value(&mut self.tab, tab, name);
                    }
                });
                ui.separator();

                match self.tab {
                    Tab::Now => self.now_tab(ui, &mut wants),
                    Tab::Event => self.event_tab(ui, &mut wants),
                    Tab::Findings => self.findings_tab(ui, &mut wants),
                    Tab::Bugs => self.bugs_tab(ui, &mut wants),
                    Tab::Log => self.log_tab(ui, &mut wants),
                }
            });

        self.carry_out(ctx, wants);
    }

    fn carry_out(&mut self, ctx: &egui::Context, wants: Wants) {
        if let Some(time) = wants.seek {
            self.playback.pause();
            self.playback.seek(time);
            self.view.reveal(time, self.loaded.duration_s);
        }
        if let Some(seq) = wants.select_event
            && let Some(index) = self.loaded.events.iter().position(|event| event.seq == seq)
        {
            self.select_event(index);
        }
        if let Some(index) = wants.select_finding
            && let Some(finding) = self.loaded.findings.get(index)
        {
            let (start, first_event) = (
                finding.start_video_s,
                finding
                    .refs
                    .iter()
                    .find_map(|reference| reference.strip_prefix("event:")?.parse::<u64>().ok()),
            );
            self.selected_finding = Some(index);
            self.playback.pause();
            self.playback.seek(start);
            self.view.reveal(start, self.loaded.duration_s);
            if let Some(seq) = first_event {
                self.selected_event = Some(seq);
            }
        }
        if let Some(id) = wants.select_bug {
            if let Some(start) = self
                .loaded
                .bugs()
                .find(|bug| bug.id == id)
                .map(|bug| bug.start_video_s)
            {
                self.playback.pause();
                self.playback.seek(start);
                self.view.reveal(start, self.loaded.duration_s);
            }
            self.selected_bug = Some(id);
        }
        if let Some(id) = wants.edit_bug {
            self.open_composer_for(&id);
        }
        if let Some(id) = wants.delete_bug {
            self.delete_annotation(&id);
        }
        if let Some((from, to)) = wants.loop_region {
            self.selection = Some((from, to));
            self.playback.seek(from);
            self.playback.set_loop(Some((from, to)));
            self.playback.play(self.playback.rate().abs());
        }
        if let Some(text) = wants.copy {
            ctx.copy_text(text);
            self.say("copied");
        }
        if wants.mark {
            self.open_composer();
        }
        if let Some(hwnd) = wants.propose_rule {
            self.open_rule_form(hwnd);
        }
    }

    /// What the OS trace recorded around an event, worked out once.
    fn os_trace_of(&mut self, seq: u64) -> Option<Arc<OsTraceData>> {
        if let Some(found) = self.os_traces.get(&seq) {
            return Some(found.clone());
        }

        let data = Arc::new(self.loaded.analysis.os_trace(seq).ok()?);
        self.os_traces.insert(seq, data.clone());
        Some(data)
    }

    /// The trace of an event, worked out once.
    fn trace_of(&mut self, seq: u64) -> Option<Arc<TraceData>> {
        if let Some(found) = self.traces.get(&seq) {
            return Some(found.clone());
        }

        let trace = Arc::new(self.loaded.analysis.trace(seq).ok()?);
        self.traces.insert(seq, trace.clone());
        Some(trace)
    }

    fn now_tab(&mut self, ui: &mut egui::Ui, wants: &mut Wants) {
        let time = self.position();
        let mach = self.loaded.analysis.manifest.mach_at_video_seconds(time);
        ui.label(RichText::new(timecode::format(time)).monospace().size(18.0));
        if let Some(wall) = self.loaded.analysis.wall_utc(mach) {
            ui.label(RichText::new(wall).small().color(theme::TEXT_DIM));
        }
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            match self.loaded.analysis.state_answer(mach, &crate::analysis::answers::StateQuery::default()) {
                Ok(state) => {
                    if let Some(focus) = &state.focus {
                        ui.label(format!(
                            "focus: window {} on display {}, workspace {}",
                            focus.window_id.map_or_else(|| "(none)".to_string(), |id| id.to_string()),
                            focus.monitor_id,
                            focus.workspace
                        ));
                    }
                    if state.paused {
                        ui.colored_label(theme::BUG, "komorebi is paused");
                    }
                    if let Some(age) = state.origin.age_ms
                        && age > 3000.0
                    {
                        ui.label(
                            RichText::new(format!(
                                "no notification for {:.0} s: komorebi can change state without one, so this may be out of date",
                                age / 1000.0
                            ))
                            .small()
                            .color(theme::TEXT_DIM),
                        );
                    }

                    ui.add_space(6.0);
                    if state.violations.is_empty() {
                        ui.label(RichText::new("nothing looks wrong in this state").color(theme::OK));
                    } else {
                        ui.label(RichText::new("wrong in this state").strong());
                        for violation in &state.violations {
                            ui.horizontal_wrapped(|ui| {
                                chip(ui, &format!("{:?}", violation.severity).to_lowercase(), theme::severity(violation.severity));
                                ui.label(&violation.message);
                            });
                        }
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new("windows").strong());
                    for monitor in &state.monitors {
                        ui.label(RichText::new(format!("display {}", monitor.id)).color(theme::TEXT_DIM));
                        for workspace in &monitor.workspaces {
                            for (index, container) in workspace.containers.iter().enumerate() {
                                for window in &container.windows {
                                    let rect = window.rect.as_ref().map_or_else(String::new, |rect| rect.describe());
                                    ui.label(
                                        RichText::new(format!("  {index}  {} \"{}\"  {rect}", window.exe, window.title))
                                            .monospace()
                                            .size(11.0),
                                    );
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    ui.label(RichText::new(error.to_string()).color(theme::TEXT_DIM));
                }
            }

            ui.add_space(10.0);
            ui.label(RichText::new("just before").strong());
            let before: Vec<usize> = (0..self.loaded.events.len())
                .filter(|index| self.loaded.events[*index].video_s <= time + 1e-3)
                .rev()
                .take(8)
                .collect();
            if before.is_empty() {
                ui.label(RichText::new("no events yet").color(theme::TEXT_DIM));
            }
            for index in before {
                let event = &self.loaded.events[index];
                ui.horizontal(|ui| {
                    chip(ui, &class_name(event.class), theme::class(event.class));
                    if ui
                        .link(RichText::new(format!("#{} {}", event.seq, event.summary)).size(12.0))
                        .clicked()
                    {
                        wants.select_event = Some(event.seq);
                    }
                    ui.label(
                        RichText::new(format!("{:.1} s ago", time - event.video_s))
                            .small()
                            .color(theme::TEXT_DIM),
                    );
                });
            }
        });
    }

    fn event_tab(&mut self, ui: &mut egui::Ui, wants: &mut Wants) {
        let Some(seq) = self.selected_event else {
            ui.label(
                RichText::new(
                    "Select an event on the timeline, or press . and , to step through them.",
                )
                .color(theme::TEXT_DIM),
            );
            return;
        };
        let Some(trace) = self.trace_of(seq) else {
            ui.label(format!("event #{seq} could not be traced"));
            return;
        };
        let event = &trace.event;

        ui.horizontal(|ui| {
            chip(ui, &class_name(event.class), theme::class(event.class));
            ui.label(RichText::new(format!("#{}  {}", event.seq, event.summary)).strong());
        });
        ui.label(RichText::new(&event.reason).small().color(theme::TEXT_DIM));
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("received {}", timecode::format(event.video_s))).monospace(),
            );
            if let Some(handler) = &event.handler {
                if ui
                    .small_button("go to its start")
                    .on_hover_text("when its handler began")
                    .clicked()
                {
                    wants.seek = Some(handler.started_video_s);
                }
                ui.label(
                    RichText::new(
                        handler
                            .handler_ms
                            .map_or_else(String::new, |ms| format!("handled in ≥ {ms:.0} ms")),
                    )
                    .small()
                    .color(theme::TEXT_DIM),
                );
            }
            if ui.small_button("go to its end").clicked() {
                wants.seek = Some(event.video_s);
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button("◀ previous").clicked()
                && let Some(index) = self
                    .loaded
                    .events
                    .iter()
                    .position(|candidate| candidate.seq == seq)
                && index > 0
            {
                wants.select_event = Some(self.loaded.events[index - 1].seq);
            }
            if ui.small_button("next ▶").clicked()
                && let Some(index) = self
                    .loaded
                    .events
                    .iter()
                    .position(|candidate| candidate.seq == seq)
                && let Some(next) = self.loaded.events.get(index + 1)
            {
                wants.select_event = Some(next.seq);
            }
            if ui.small_button("copy trace command").clicked() {
                wants.copy = Some(format!(
                    "komorebi-recorder trace '{}' event:{}",
                    self.loaded.dir.display(),
                    seq
                ));
            }
        });
        ui.separator();

        let os_trace = self.os_trace_of(seq);
        egui::ScrollArea::vertical().show(ui, |ui| {
            if !trace.notes.is_empty() {
                for note in &trace.notes {
                    ui.label(
                        RichText::new(format!("• {note}"))
                            .small()
                            .color(Color32::from_rgb(235, 185, 60)),
                    );
                }
                ui.add_space(6.0);
            }

            ui.label(RichText::new(format!("what changed ({})", trace.changes.len())).strong());
            if trace.changes.is_empty() {
                ui.label(RichText::new("the state did not change").color(theme::TEXT_DIM));
            }
            for traced in &trace.changes {
                ui.horizontal_wrapped(|ui| {
                    chip(
                        ui,
                        &traced.attribution.actor,
                        actor_colour(&traced.attribution.actor),
                    );
                    ui.label(
                        RichText::new(&traced.attribution.confidence)
                            .small()
                            .color(theme::TEXT_DIM),
                    );
                    ui.label(&traced.change.text);
                });
                ui.label(
                    RichText::new(format!("    {}", traced.attribution.reason))
                        .small()
                        .color(theme::TEXT_DIM),
                );
                if !traced.attribution.evidence.is_empty() {
                    let lines: Vec<String> = traced
                        .attribution
                        .evidence
                        .iter()
                        .map(|line| format!("log:{line}"))
                        .collect();
                    ui.label(
                        RichText::new(format!("    evidence {}", lines.join(" ")))
                            .small()
                            .monospace()
                            .color(theme::TEXT_DIM),
                    );
                }
            }

            for (heading, list) in [
                (
                    "wrong after this event, and not before",
                    &trace.violations_appeared,
                ),
                (
                    "wrong before this event, and not after",
                    &trace.violations_resolved,
                ),
            ] {
                if list.is_empty() {
                    continue;
                }
                ui.add_space(6.0);
                ui.label(RichText::new(heading).strong());
                for violation in list {
                    ui.horizontal_wrapped(|ui| {
                        chip(
                            ui,
                            &format!("{:?}", violation.severity).to_lowercase(),
                            theme::severity(violation.severity),
                        );
                        ui.label(&violation.message);
                    });
                }
            }

            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("what the handler logged ({})", trace.handler.len()))
                    .strong(),
            );
            for line in &trace.handler {
                log_line_ui(ui, line, wants);
            }
            if !trace.ambient.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "logged by other threads meanwhile ({})",
                        trace.ambient.len()
                    ))
                    .strong(),
                );
                for line in &trace.ambient {
                    log_line_ui(ui, line, wants);
                }
            }

            if let Some(os) = &os_trace {
                ui.add_space(8.0);
                os_trace_ui(ui, &self.loaded.dir, os, wants);
            }
        });
    }

    fn findings_tab(&mut self, ui: &mut egui::Ui, wants: &mut Wants) {
        ui.label(
            RichText::new(format!(
                "{} things that look wrong or unusual, most worth looking at first",
                self.loaded.findings.len()
            ))
            .small()
            .color(theme::TEXT_DIM),
        );
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, finding) in self.loaded.findings.iter().enumerate() {
                let selected = self.selected_finding == Some(index);
                let frame = egui::Frame::new()
                    .fill(if selected {
                        Color32::from_rgb(38, 42, 56)
                    } else {
                        theme::LANE
                    })
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::same(6));
                let response = frame
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            chip(
                                ui,
                                &format!("{:?}", finding.severity).to_lowercase(),
                                theme::severity(finding.severity),
                            );
                            ui.label(
                                RichText::new(timecode::format(finding.start_video_s)).monospace(),
                            );
                            ui.label(RichText::new(&finding.kind).small().color(theme::TEXT_DIM));
                            if !finding.bugs.is_empty() {
                                chip(ui, "near the bug", theme::BUG);
                            }
                        });
                        ui.label(&finding.summary);
                    })
                    .response
                    .interact(egui::Sense::click());
                if response.clicked() {
                    wants.select_finding = Some(index);
                }
                ui.add_space(3.0);
            }
        });
    }

    fn bugs_tab(&mut self, ui: &mut egui::Ui, wants: &mut Wants) {
        if ui
            .add(
                egui::Button::new(RichText::new("Mark a bug region…").color(Color32::WHITE))
                    .fill(Color32::from_rgb(150, 40, 40)),
            )
            .clicked()
        {
            wants.mark = true;
        }
        ui.label(
            RichText::new(
                "The region you mark, and what you say is wrong, is where an investigation starts.",
            )
            .small()
            .color(theme::TEXT_DIM),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut any = false;
            for kind in [Kind::Bug, Kind::Finding, Kind::Note] {
                for annotation in self.loaded.annotations.iter().filter(|annotation| annotation.kind == kind) {
                    any = true;
                    let selected = self.selected_bug.as_deref() == Some(annotation.id.as_str());
                    egui::Frame::new()
                        .fill(if selected { Color32::from_rgb(52, 32, 34) } else { theme::LANE })
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let colour = match kind {
                                    Kind::Bug => theme::BUG,
                                    Kind::Finding => theme::OK,
                                    Kind::Note => theme::TEXT_DIM,
                                };
                                chip(ui, &format!("{}", annotation.kind), colour);
                                ui.label(
                                    RichText::new(format!(
                                        "{} – {}{}",
                                        timecode::format(annotation.start_video_s),
                                        timecode::format(annotation.end_video_s),
                                        if annotation.open_ended { " (open-ended)" } else { "" }
                                    ))
                                    .monospace(),
                                );
                                ui.label(RichText::new(format!("by {}", annotation.author)).small().color(theme::TEXT_DIM));
                            });
                            if !annotation.title.is_empty() {
                                ui.label(RichText::new(&annotation.title).strong());
                            }
                            if annotation.needs_description {
                                ui.colored_label(Color32::from_rgb(235, 185, 60), "needs a description: an investigator reads what you wrote here");
                            } else {
                                ui.label(&annotation.text);
                            }
                            if let Some(expected) = &annotation.expected {
                                ui.label(RichText::new(format!("expected: {expected}")).color(theme::TEXT_DIM));
                            }
                            if !annotation.refs.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new("evidence").small().color(theme::TEXT_DIM));
                                    for reference in &annotation.refs {
                                        let text = reference.to_string();
                                        if let crate::annotations::Ref::Event { seq } = reference {
                                            if ui.link(&text).clicked() {
                                                wants.select_event = Some(*seq);
                                            }
                                        } else if let crate::annotations::Ref::Frame { video_s } = reference {
                                            if ui.link(&text).clicked() {
                                                wants.seek = Some(*video_s);
                                            }
                                        } else {
                                            ui.label(RichText::new(text).monospace().small());
                                        }
                                    }
                                });
                            }
                            ui.horizontal(|ui| {
                                if ui.small_button("go to").clicked() {
                                    wants.select_bug = Some(annotation.id.clone());
                                }
                                if ui.small_button("loop it").clicked() {
                                    wants.loop_region = Some((annotation.start_video_s, annotation.end_video_s));
                                }
                                if kind == Kind::Bug && ui.small_button("edit").clicked() {
                                    wants.edit_bug = Some(annotation.id.clone());
                                }
                                if ui.small_button("delete").clicked() {
                                    wants.delete_bug = Some(annotation.id.clone());
                                }
                            });
                        });
                    if kind == Kind::Bug {
                        for ghost in self.ghosts_during(annotation.start_video_s, annotation.end_video_s) {
                            ghost_card(ui, &ghost, wants);
                        }
                    }
                    ui.add_space(4.0);
                }
            }

            if !any {
                ui.label(RichText::new("Nothing marked yet. Press B, or the Mark bug button.").color(theme::TEXT_DIM));
            }

            if self.loaded.bugs().next().is_none() {
                return;
            }
            ui.add_space(8.0);
            if ui.button("Copy a prompt for an agent").on_hover_text("a ready-to-paste instruction that starts an investigation from what you marked").clicked() {
                wants.copy = Some(agent_prompt(&self.loaded.dir));
            }
        });
    }

    fn log_tab(&mut self, ui: &mut egui::Ui, wants: &mut Wants) {
        let log = self.loaded.analysis.log();
        let times = &self.loaded.log_times;
        if log.lines.is_empty() {
            ui.label(RichText::new("komorebi's log was not recorded").color(theme::TEXT_DIM));
            return;
        }

        // the line the playhead is at: the last one at or before it
        let position = self.position();
        let current = times
            .partition_point(|time| time.is_nan() || *time <= position + 1e-3)
            .saturating_sub(1);

        let row_height = 15.0;
        let area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        let area = if self.playback.is_playing() {
            // follow the playhead while playing
            area.vertical_scroll_offset((current as f32 - 12.0).max(0.0) * row_height)
        } else {
            area
        };
        area.show_rows(ui, row_height, log.lines.len(), |ui, range| {
            for index in range {
                let line = &log.lines[index];
                let time = times.get(index).copied().unwrap_or(f64::NAN);
                let colour = line.level.map_or(theme::TEXT_DIM, |level| match level {
                    crate::analysis::logparse::Level::Error
                    | crate::analysis::logparse::Level::Warn => theme::level(level),
                    _ => Color32::from_gray(190),
                });
                let text = format!(
                    "{} {:>5} {}",
                    if time.is_nan() {
                        "        ".to_string()
                    } else {
                        timecode::format(time)
                    },
                    line.level
                        .map_or(String::new(), |level| format!("{level:?}").to_uppercase()),
                    line.message
                );
                let label = RichText::new(text).monospace().size(10.5).color(colour);
                let response = ui.selectable_label(index == current, label);
                if response.clicked() && !time.is_nan() {
                    wants.seek = Some(time);
                }
            }
        });
    }
}

fn log_line_ui(ui: &mut egui::Ui, line: &crate::analysis::trace::TraceLine, wants: &mut Wants) {
    let colour = match line.level.as_deref() {
        Some("ERROR") => theme::level(crate::analysis::logparse::Level::Error),
        Some("WARN") => theme::level(crate::analysis::logparse::Level::Warn),
        _ => Color32::from_gray(190),
    };
    ui.horizontal_wrapped(|ui| {
        if let Some(time) = line.video_s
            && ui
                .link(RichText::new(timecode::format(time)).monospace().size(10.5))
                .on_hover_text("go there")
                .clicked()
        {
            wants.seek = Some(time);
        }
        ui.label(
            RichText::new(format!(
                "{} {}",
                line.level.as_deref().unwrap_or(""),
                line.message
            ))
            .monospace()
            .size(10.5)
            .color(colour),
        );
    });
    if let Some(source) = &line.source {
        let mut text = format!("      {}  in {}", source.location, source.function);
        if source.file_changed {
            text.push_str("  [file differs from the recorded komorebi]");
        }
        if line.changes_state_silently {
            text.push_str("  [changes state without a notification]");
        }
        let response = ui.label(
            RichText::new(text)
                .monospace()
                .size(10.0)
                .color(theme::TEXT_DIM),
        );
        if response.clicked() {
            wants.copy = Some(source.location.clone());
        }
        response.on_hover_text("source of this log line");
    }
}

/// What the rest of the system did around an event, one collapsible group per source.
fn os_trace_ui(ui: &mut egui::Ui, dir: &std::path::Path, os: &OsTraceData, wants: &mut Wants) {
    ui.label(RichText::new(format!("the system around it ({})", os.total)).strong());
    ui.label(
        RichText::new(format!(
            "from {} (its handler began) to {}",
            timecode::format(os.started_s),
            timecode::format(os.until_s)
        ))
        .small()
        .color(theme::TEXT_DIM),
    );
    for note in &os.notes {
        ui.label(
            RichText::new(format!("• {note}"))
                .small()
                .color(Color32::from_rgb(235, 185, 60)),
        );
    }

    for group in &os.groups {
        let colour = match group.kind {
            GroupKind::Komorebi => theme::ACCENT,
            GroupKind::System => theme::INCIDENT,
            GroupKind::App => theme::OK,
            GroupKind::Other => theme::TEXT_DIM,
        };
        let title = format!(
            "{}  {} during, {} after",
            group.label, group.during, group.after
        );
        egui::CollapsingHeader::new(RichText::new(title).color(colour))
            .id_salt((os.seq, &group.source, &group.label))
            .default_open(group.kind == GroupKind::Komorebi && group.total() <= 12)
            .show(ui, |ui| {
                for record in &group.records {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .link(
                                RichText::new(timecode::format(record.video_s))
                                    .monospace()
                                    .size(10.5),
                            )
                            .on_hover_text("go there")
                            .clicked()
                        {
                            wants.seek = Some(record.video_s);
                        }
                        ui.label(
                            RichText::new(match record.phase {
                                Phase::During => " ",
                                Phase::After => "+",
                            })
                            .monospace()
                            .small()
                            .color(theme::TEXT_DIM),
                        )
                        .on_hover_text(match record.phase {
                            Phase::During => "while komorebi's handler was running",
                            Phase::After => "after the notification was sent",
                        });
                        ui.label(
                            RichText::new(&record.text)
                                .monospace()
                                .size(10.5)
                                .color(Color32::from_gray(190)),
                        );
                    });
                }

                if group.omitted() > 0 {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("… {} more", group.omitted()))
                                .small()
                                .color(theme::TEXT_DIM),
                        );
                        if ui.small_button("copy command").clicked() {
                            wants.copy = Some(format!(
                                "komorebi-recorder at '{}' {:.3} --window 1s --source '{}'",
                                dir.display(),
                                os.received_s,
                                group.source
                            ));
                        }
                    });
                }
            });
    }

    if !os.overlapping_events.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("other events in this window:")
                    .small()
                    .color(theme::TEXT_DIM),
            );
            for seq in &os.overlapping_events {
                if ui.small_button(format!("#{seq}")).clicked() {
                    wants.select_event = Some(*seq);
                }
            }
        });
    }
}

/// Returns a prompt that starts an agent's investigation of the session in `dir` from what the user
/// marked.
///
/// The prompt names the session and tells the agent to begin with `komorebi-recorder brief` on it
/// (where the user's description of the bug is the ground truth) and to read `komorebi-recorder
/// guide` for how to work from there to the cause. The Bugs tab copies it to the clipboard.
///
/// This lives in a module that is not public, so it has no example; the test
/// `the_agent_prompt_names_the_session_and_the_commands_to_start_with` in `viewer::app` pins what it
/// contains.
#[must_use]
pub fn agent_prompt(dir: &std::path::Path) -> String {
    let dir = dir.display();
    format!(
        "Investigate the komorebi bug recorded in this session: {dir}\n\n\
         Start with `komorebi-recorder brief '{dir}'`: the user marked where the bug starts and ends \
         and described what is wrong, and that description is the ground truth. Read \
         `komorebi-recorder guide` for how to work from there to the cause, then propose a fix \
         grounded in what the recording shows, citing evidence by event, log line and source line."
    )
}
