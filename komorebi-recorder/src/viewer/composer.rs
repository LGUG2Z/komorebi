//! Marking a bug region: where it starts and ends, and what is wrong.
//!
//! This is the most important thing the viewer writes. The region and the words are what an
//! investigation starts from, so the form asks for the words, makes the times easy to take from the
//! video, and shows plainly that a region without a description is not much use.
//!
//! A [`Composer`] is the form's state: the text boxes as typed, so what is in them is not yet a
//! time or a region. [`Open::composer_window`] draws it, and [`Open::save_composer`] checks it and
//! writes the region to the session's annotations file (the same append-only `annotations.jsonl`
//! the command line reads, through [`annotations::add`] and [`annotations::edit`]). The same file
//! is also changed by what the other methods here do to a region once it exists: edit it
//! ([`Open::open_composer_for`]), move an edge of it ([`Open::move_bug_edge`]), delete it and bring
//! it back ([`Open::delete_annotation`], [`Open::undo_delete`]).
//!
//! This module is the only part of the viewer that writes to the session, and only to its
//! annotations. It needs an `egui::Context` to draw, so there are no examples; the tests in `viewer::app` fill
//! the form in and save it (`marking_a_bug_from_a_selection_saves_a_region_anyone_can_read`), cover
//! what a form is refused for (`a_region_the_form_cannot_make_sense_of_stays_open_and_says_why`),
//! open-ended regions (`a_region_can_be_open_ended`), and editing, dragging, deleting and bringing
//! one back (`a_region_can_be_edited_dragged_deleted_and_brought_back`).

use super::app::Open;
use super::app::Tab;
use super::theme;
use crate::analysis::envelope;
use crate::annotations;
use crate::annotations::AnnotationView;
use crate::annotations::Kind;
use crate::annotations::New;
use crate::annotations::Patch;
use crate::timecode;
use eframe::egui;

/// The form for one bug region.
///
/// Every field is what is in a box of the form, as typed: the times are timecodes in text, and
/// nothing is checked until [`Open::save_composer`] reads the form.
pub struct Composer {
    /// The id of the region being changed; `None` for a new one.
    pub editing: Option<String>,
    /// An optional short title for the region.
    pub title: String,
    /// The description of what goes wrong, in the user's words. It is required: a region without
    /// one is refused.
    pub text: String,
    /// A description of what should have happened instead. Optional.
    pub expected: String,
    /// Where the region starts, as a timecode as typed (`00:03.000`, or any form
    /// [`timecode::parse`] reads, such as `3.5` or `1:02`).
    pub start: String,
    /// Where the region ends, as a timecode as typed. Ignored while [`Composer::open_ended`] is set.
    pub end: String,
    /// It was still going on when the recording ended.
    pub open_ended: bool,
    /// Which displays it was seen on, as `(display id, ticked)`; none ticked means any.
    pub displays: Vec<(u32, bool)>,
    /// Why the last attempt to save was refused, shown in the form.
    pub error: Option<String>,
    /// Focus the description when the form first shows.
    first: bool,
}

impl Composer {
    /// Returns a blank form for a region from `start_s` to `end_s`, in session video seconds.
    ///
    /// The times are shown as timecodes. `displays` are the ids of the displays offered to tick,
    /// none of them ticked.
    #[must_use]
    pub fn new(start_s: f64, end_s: f64, displays: &[u32]) -> Self {
        Self {
            editing: None,
            title: String::new(),
            text: String::new(),
            expected: String::new(),
            start: timecode::format(start_s),
            end: timecode::format(end_s),
            open_ended: false,
            displays: displays.iter().map(|id| (*id, false)).collect(),
            error: None,
            first: true,
        }
    }

    /// Returns the form for changing the existing region `bug`, filled in with its values.
    ///
    /// `displays` are the ids of the displays offered to tick; the ones `bug` was seen on are
    /// ticked.
    #[must_use]
    pub fn editing(bug: &AnnotationView, displays: &[u32]) -> Self {
        Self {
            editing: Some(bug.id.clone()),
            title: bug.title.clone(),
            text: bug.text.clone(),
            expected: bug.expected.clone().unwrap_or_default(),
            start: timecode::format(bug.start_video_s),
            end: timecode::format(bug.end_video_s),
            open_ended: bug.open_ended,
            displays: displays
                .iter()
                .map(|id| (*id, bug.displays.contains(id)))
                .collect(),
            error: None,
            first: true,
        }
    }
}

impl Open {
    /// Opens the form for a new region, and shows the Bugs tab.
    ///
    /// The region is the selection if there is one, else from the playhead to five seconds after it
    /// (or to the end of the recording, if that comes first).
    pub fn open_composer(&mut self) {
        let duration = self.loaded.duration_s;
        let (start, end) = self.selection.unwrap_or_else(|| {
            let position = self.position();
            (position, (position + 5.0).min(duration))
        });
        let displays: Vec<u32> = self
            .loaded
            .displays
            .iter()
            .map(|display| display.id)
            .collect();
        self.composer = Some(Composer::new(start, end, &displays));
        self.tab = Tab::Bugs;
    }

    /// Opens the form for changing the bug region `id`.
    ///
    /// Nothing happens if there is no bug region with that id. A note or a finding is not a bug
    /// region and cannot be changed here.
    pub fn open_composer_for(&mut self, id: &str) {
        let displays: Vec<u32> = self
            .loaded
            .displays
            .iter()
            .map(|display| display.id)
            .collect();
        if let Some(bug) = self.loaded.bugs().find(|bug| bug.id == id) {
            self.composer = Some(Composer::editing(bug, &displays));
        }
    }

    /// Writes the form to `annotations.jsonl`: a new bug region, or a change to the one being edited.
    ///
    /// On success the annotations are read again, the saved region is selected, the selection on the
    /// timeline is cleared, the form is closed and the top bar shows that it was saved. Nothing happens,
    /// and `Ok` is returned, if no form is open. A new region is written with the user as its
    /// author, and ticked displays are recorded as where it was seen (none ticked: any).
    ///
    /// The start (and the end, unless the form's `open_ended` box is ticked) is read with
    /// [`timecode::parse`] and may be up to a millisecond outside the recording, which is then
    /// clamped to it. An open-ended region has no end. Editing can set an end but not take one
    /// away: ticking "still happening" on a region that already has an end leaves its end as it was.
    ///
    /// # Errors
    ///
    /// Returns a message, fit to show a person, and leaves the form open and nothing written, if
    ///
    /// - the start is not a time ("the start is not a time like 12.5 or 0:12.500"), or the end is
    ///   not one and the region is not open-ended,
    /// - a time is outside the recording ("the start is outside the recording (0 to 00:10.000)"),
    /// - the end is before the start ("the end is before the start"), or
    /// - the description is empty, after trimming ("say what goes wrong: whoever investigates reads
    ///   this first").
    ///
    /// It also returns a message, with the cause, if the annotations file cannot be written
    /// (including a region that does not exist, when editing) or, after the region has been
    /// written, cannot be read again.
    ///
    /// Covered by `a_region_the_form_cannot_make_sense_of_stays_open_and_says_why` and
    /// `a_region_can_be_open_ended`.
    pub fn save_composer(&mut self) -> Result<(), String> {
        let Some(form) = &self.composer else {
            return Ok(());
        };

        let manifest = &self.loaded.analysis.manifest;
        let duration = self.loaded.duration_s;
        let parse = |what: &str, text: &str| -> Result<f64, String> {
            timecode::parse(text)
                .map_err(|_| format!("{what} is not a time like 12.5 or 0:12.500"))
                .and_then(|seconds| {
                    if (-0.001..=duration + 0.001).contains(&seconds) {
                        Ok(seconds.clamp(0.0, duration))
                    } else {
                        Err(format!(
                            "{what} is outside the recording (0 to {})",
                            timecode::format(duration)
                        ))
                    }
                })
        };

        let start_s = parse("the start", &form.start)?;
        let end_s = if form.open_ended {
            None
        } else {
            Some(parse("the end", &form.end)?)
        };
        if let Some(end) = end_s
            && end < start_s
        {
            return Err("the end is before the start".to_string());
        }
        let text = form.text.trim().to_string();
        if text.is_empty() {
            return Err("say what goes wrong: whoever investigates reads this first".to_string());
        }

        let displays: Vec<u32> = form
            .displays
            .iter()
            .filter(|(_, ticked)| *ticked)
            .map(|(id, _)| *id)
            .collect();
        let expected = form.expected.trim().to_string();
        let start_ns = manifest.mach_at_video_seconds(start_s);
        let end_ns = end_s.map(|end| manifest.mach_at_video_seconds(end));

        let dir = self.loaded.dir.clone();
        let saved = match &form.editing {
            Some(id) => {
                let id = id.clone();
                annotations::edit(
                    &dir,
                    &id,
                    Patch {
                        start_mach_ns: Some(start_ns),
                        end_mach_ns: end_ns,
                        title: Some(form.title.trim().to_string()),
                        text: Some(text),
                        expected: Some(expected),
                        displays: Some(displays),
                        refs: None,
                    },
                )
                .map(|()| id)
            }
            None => annotations::add(
                &dir,
                New {
                    kind: Some(Kind::Bug),
                    start_mach_ns: start_ns,
                    end_mach_ns: end_ns,
                    title: form.title.trim().to_string(),
                    text,
                    expected: (!expected.is_empty()).then_some(expected),
                    displays,
                    author: Some(annotations::AUTHOR_USER.to_string()),
                    about: None,
                    refs: Vec::new(),
                },
            ),
        }
        .map_err(|error| format!("{error:#}"))?;

        self.loaded
            .reload_annotations()
            .map_err(|error| format!("{error:#}"))?;
        self.selected_bug = Some(saved);
        self.selection = None;
        self.composer = None;
        self.say("saved: this is what an investigation will start from");
        Ok(())
    }

    /// Deletes the annotation `id` (a bug region, note or finding), keeping a copy to bring back.
    ///
    /// The top bar shows that it was deleted and that `Command`-`Z` brings it back. The line stays in the
    /// annotations file as a tombstone (see [`annotations::delete`]). Nothing happens if there is no
    /// such annotation. If the file cannot be written the top bar shows why, and nothing is deleted.
    pub fn delete_annotation(&mut self, id: &str) {
        let Some(gone) = self
            .loaded
            .annotations
            .iter()
            .find(|annotation| annotation.id == id)
            .cloned()
        else {
            return;
        };

        match annotations::delete(&self.loaded.dir, id) {
            Ok(()) => {
                let _ = self.loaded.reload_annotations();
                if self.selected_bug.as_deref() == Some(id) {
                    self.selected_bug = None;
                }
                self.deleted.push(gone);
                self.say("deleted (⌘Z brings it back)");
            }
            Err(error) => self.say(format!("could not delete: {error:#}")),
        }
    }

    /// Brings back the annotation deleted last.
    ///
    /// It is added to the annotations file again with everything it had (kind, times, title, text,
    /// what was expected, displays, author, what it is about and its references), so it has a new
    /// id, and it is selected. Nothing happens if nothing was deleted. If it cannot be added the top
    /// bar shows why.
    pub fn undo_delete(&mut self) {
        let Some(back) = self.deleted.pop() else {
            return;
        };
        let manifest = &self.loaded.analysis.manifest;
        let result = annotations::add(
            &self.loaded.dir,
            New {
                kind: Some(back.kind),
                start_mach_ns: back.start_mach_ns,
                end_mach_ns: back.end_mach_ns,
                title: back.title.clone(),
                text: back.text.clone(),
                expected: back.expected.clone(),
                displays: back.displays.clone(),
                author: Some(back.author.clone()),
                about: back.about.clone(),
                refs: back.refs.clone(),
            },
        );
        let _ = manifest;
        match result {
            Ok(id) => {
                let _ = self.loaded.reload_annotations();
                self.selected_bug = Some(id);
                self.say("brought back");
            }
            Err(error) => self.say(format!("could not bring it back: {error:#}")),
        }
    }

    /// Moves one edge of the bug region `id` to `to_s` seconds, from dragging it on the timeline.
    ///
    /// `moving_start` says which edge: the start if `true`, the end if `false`. `to_s` is clamped
    /// to the recording. The annotations are read again afterwards.
    ///
    /// # Errors
    ///
    /// Returns a message, fit to show a person, if
    ///
    /// - the change would put the end before the start ("a region cannot end before it starts"), or
    /// - the annotations file cannot be written (including a region that does not exist) or cannot
    ///   be read again, which gives the cause.
    ///
    /// Covered by `a_region_can_be_edited_dragged_deleted_and_brought_back`.
    pub fn move_bug_edge(&mut self, id: &str, moving_start: bool, to_s: f64) -> Result<(), String> {
        let manifest = &self.loaded.analysis.manifest;
        let ns = manifest.mach_at_video_seconds(to_s.clamp(0.0, self.loaded.duration_s));
        let patch = if moving_start {
            Patch {
                start_mach_ns: Some(ns),
                ..Patch::default()
            }
        } else {
            Patch {
                end_mach_ns: Some(ns),
                ..Patch::default()
            }
        };

        annotations::edit(&self.loaded.dir, id, patch).map_err(
            |error| match envelope::exit_kind(&error) {
                envelope::ExitKind::Usage => "a region cannot end before it starts".to_string(),
                _ => format!("{error:#}"),
            },
        )?;
        self.loaded
            .reload_annotations()
            .map_err(|error| format!("{error:#}"))
    }

    /// Draws the form, if one is open, as a window in the middle of the screen.
    ///
    /// The window is titled "Mark a bug region", or "Edit bug region" for an existing one. It asks
    /// for what goes wrong, what was expected and a short title, and for the times, each with a
    /// button that takes the playhead's time and one that shows that moment. Save calls
    /// [`Open::save_composer`] and, if it is refused, shows the reason in the form. Cancel, or
    /// closing the window, drops the form without saving anything.
    ///
    /// Needs an `egui::Context`, so a window is needed.
    pub fn composer_window(&mut self, ctx: &egui::Context) {
        let Some(form) = self.composer.as_mut() else {
            return;
        };

        let position = self.playback.position();
        let mut save = false;
        let mut cancel = false;
        let mut seek: Option<String> = None;
        let mut open = true;
        let title = if form.editing.is_some() {
            "Edit bug region"
        } else {
            "Mark a bug region"
        };

        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Mark the stretch of the recording where the bug is visible, and say what it looks like. \
                         Whoever investigates it starts from exactly this.",
                    )
                    .color(theme::TEXT_DIM),
                );
                ui.add_space(8.0);

                ui.label("What goes wrong");
                let description = ui.add(
                    egui::TextEdit::multiline(&mut form.text)
                        .hint_text("e.g. after unplugging the monitor, my browser window disappears instead of moving to the laptop screen")
                        .desired_rows(4)
                        .desired_width(f32::INFINITY),
                );
                if form.first {
                    description.request_focus();
                    form.first = false;
                }

                ui.add_space(4.0);
                ui.label("What you expected instead (optional)");
                ui.add(
                    egui::TextEdit::multiline(&mut form.expected)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(4.0);
                ui.label("A short title (optional)");
                ui.add(egui::TextEdit::singleline(&mut form.title).desired_width(f32::INFINITY));

                ui.add_space(8.0);
                egui::Grid::new("times").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("starts");
                    ui.add(egui::TextEdit::singleline(&mut form.start).desired_width(90.0).font(egui::TextStyle::Monospace));
                    if ui.button("= playhead").clicked() {
                        form.start = timecode::format(position);
                    }
                    if ui.button("go").on_hover_text("show this moment").clicked() {
                        seek = Some(form.start.clone());
                    }
                    ui.end_row();

                    ui.label("ends");
                    ui.add_enabled(
                        !form.open_ended,
                        egui::TextEdit::singleline(&mut form.end).desired_width(90.0).font(egui::TextStyle::Monospace),
                    );
                    if ui.add_enabled(!form.open_ended, egui::Button::new("= playhead")).clicked() {
                        form.end = timecode::format(position);
                    }
                    if ui.add_enabled(!form.open_ended, egui::Button::new("go")).clicked() {
                        seek = Some(form.end.clone());
                    }
                    ui.end_row();
                });
                ui.checkbox(&mut form.open_ended, "still happening when the recording ended");

                if form.displays.len() > 1 {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("seen on");
                        for (id, ticked) in &mut form.displays {
                            ui.checkbox(ticked, format!("display {id}"));
                        }
                        ui.label(egui::RichText::new("(none ticked: any)").small().color(theme::TEXT_DIM));
                    });
                }

                if let Some(error) = &form.error {
                    ui.add_space(6.0);
                    ui.colored_label(theme::BUG, error);
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(egui::RichText::new("Save").color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(150, 40, 40)))
                        .clicked()
                    {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if let Some(text) = seek
            && let Ok(seconds) = timecode::parse(&text)
        {
            self.playback.pause();
            self.playback.seek(seconds);
        }
        if cancel || !open {
            self.composer = None;
        } else if save
            && let Err(message) = self.save_composer()
            && let Some(form) = self.composer.as_mut()
        {
            form.error = Some(message);
        }
    }
}
