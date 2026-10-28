//! Proposing a rule for a ghost tile: the form that shows what would be sent, and the sending.
//!
//! When a bug region the user marked was made while a ghost tile was there
//! ([`crate::analysis::ghost`]), the Bugs tab offers to propose a rule that ignores that window,
//! to komorebi-application-specific-configuration ([`crate::asc`]). The button opens a
//! [`RuleForm`]. It shows what was seen and the rule that would be proposed, with the application's
//! name to change, and says plainly what sending does: nothing leaves the machine until the user
//! presses the button.
//!
//! Pressing it sends in the background ([`Open::start_sending`]): cloning, committing and pushing
//! take seconds, and a window that stops answering for them is not one anyone trusts with their
//! credentials. The form lists the steps as they happen, and ends with what [`submit`] came to: the
//! link to the pull request, a link to open one, or, if something failed, where the clone is kept
//! and what to do to finish by hand.
//!
//! This needs an `egui::Context` to draw, so there are no examples; the tests in `viewer::app` open
//! the form, send to a local repository and read what came back.

use super::app::Open;
use super::theme;
use crate::analysis::ghost::GhostTile;
use crate::asc::Platform;
use crate::asc::rule::Rule;
use crate::asc::rule::app_name;
use crate::asc::submit;
use crate::asc::submit::Outcome;
use crate::asc::submit::Proposal;
use crate::asc::submit::Tools;
use crate::asc::submit::Upstream;
use crate::timecode;
use eframe::egui;
use eframe::egui::RichText;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::channel;

/// What the background thread tells the form.
pub enum Update {
    /// A step has begun.
    Said(String),
    /// It is over, and this is how it went.
    Finished(Outcome),
}

/// Where the form is.
pub enum Stage {
    /// Showing what would be sent, waiting for the user.
    Review,
    /// Sending: the steps so far, and the channel the rest arrives on.
    Sending {
        /// The steps that have begun, in order.
        said: Vec<String>,
        /// Where the thread reports.
        updates: Receiver<Update>,
    },
    /// Over.
    Done(Outcome),
}

/// The form for proposing the rule for one ghost tile.
pub struct RuleForm {
    /// The ghost tile the rule is for.
    pub ghost: GhostTile,
    /// The name of the application's entry in the rule file, as typed.
    pub app: String,
    /// Where it is.
    pub stage: Stage,
}

impl RuleForm {
    /// A form for `ghost`, with the application named for its program.
    #[must_use]
    pub fn new(ghost: GhostTile) -> Self {
        Self {
            app: app_name(&ghost.exe),
            ghost,
            stage: Stage::Review,
        }
    }

    /// The rule the form would send: the narrowest [`Rule::for_ghost`] can write, as the
    /// application the form names.
    ///
    /// # Errors
    ///
    /// Returns what is missing when the name is blank, or when there is not enough to write a rule
    /// that catches the ghost and nothing else.
    pub fn rule(&self) -> Result<Rule, String> {
        let app = self.app.trim();
        if app.is_empty() {
            return Err("name the application".to_string());
        }
        let mut rule = Rule::for_ghost(&self.ghost).map_err(|error| error.to_string())?;
        rule.app = app.to_string();
        Ok(rule)
    }
}

impl Open {
    /// The ghost tiles that were there at some time between `start_s` and `end_s` of the video,
    /// which is what a bug region marked then is about.
    #[must_use]
    pub fn ghosts_during(&self, start_s: f64, end_s: f64) -> Vec<GhostTile> {
        self.loaded
            .ghosts
            .iter()
            .filter(|ghost| ghost.overlaps(start_s, end_s, self.loaded.duration_s))
            .cloned()
            .collect()
    }

    /// Opens the form for the ghost tile with window handle `hwnd`, if the recording has one.
    pub fn open_rule_form(&mut self, hwnd: u64) {
        if let Some(ghost) = self.loaded.ghosts.iter().find(|ghost| ghost.hwnd == hwnd) {
            self.rule = Some(RuleForm::new(ghost.clone()));
        }
    }

    /// Sends the rule the form shows, in the background, with the tools and the repository `make`
    /// gives (it runs on the thread: finding `gh` runs it).
    ///
    /// Does nothing unless the form is open, at `Stage::Review`, and has a rule to send. The
    /// window sends to the real repository with [`Open::send_rule`]; this is its seam for the tests.
    pub fn start_sending(&mut self, make: impl FnOnce() -> (Tools, Upstream) + Send + 'static) {
        let Some(form) = self.rule.as_mut() else {
            return;
        };
        let (Stage::Review, Ok(rule)) = (&form.stage, form.rule()) else {
            return;
        };

        let proposal = Proposal::ghost_tile(&form.ghost, rule, env!("CARGO_PKG_VERSION"));
        let (sender, updates) = channel();
        std::thread::spawn(move || {
            let (tools, upstream) = make();
            let progress = sender.clone();
            let outcome = submit::submit(&proposal, &tools, &upstream, &mut |step| {
                let _ = progress.send(Update::Said(step.to_string()));
            });
            let _ = sender.send(Update::Finished(outcome));
        });
        form.stage = Stage::Sending {
            said: Vec::new(),
            updates,
        };
    }

    /// Sends the rule to komorebi-application-specific-configuration on GitHub.
    pub fn send_rule(&mut self) {
        self.start_sending(|| (Tools::detect(), Upstream::komorebi_asc()));
    }

    /// Takes in what the sending thread has reported.
    pub fn poll_rule(&mut self) {
        let Some(form) = self.rule.as_mut() else {
            return;
        };
        let mut finished = None;
        if let Stage::Sending { said, updates } = &mut form.stage {
            while let Ok(update) = updates.try_recv() {
                match update {
                    Update::Said(step) => said.push(step),
                    Update::Finished(outcome) => finished = Some(outcome),
                }
            }
        }
        if let Some(outcome) = finished {
            form.stage = Stage::Done(outcome);
        }
    }

    /// Whether a sending is going on, so that the window asks to be drawn again to see it end.
    #[must_use]
    pub fn is_sending(&self) -> bool {
        self.rule
            .as_ref()
            .is_some_and(|form| matches!(form.stage, Stage::Sending { .. }))
    }

    /// Draws the form, if it is open. Needs an `egui::Context`, so a window is needed.
    pub fn rule_window(&mut self, ctx: &egui::Context) {
        self.poll_rule();
        let Some(form) = self.rule.as_mut() else {
            return;
        };

        let mut open = true;
        let mut send = false;
        let mut copy: Option<String> = None;
        let mut reveal: Option<std::path::PathBuf> = None;
        let sending = matches!(form.stage, Stage::Sending { .. });

        egui::Window::new("Propose a rule for this ghost tile")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(540.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let ghost = form.ghost.clone();
                ui.label(RichText::new(what_was_seen(&ghost)).color(theme::TEXT_DIM));
                ui.add_space(8.0);

                match &form.stage {
                    Stage::Review => {
                        ui.label(RichText::new("The rule").strong());
                        ui.label(
                            RichText::new(format!(
                                "for {} in komorebi-application-specific-configuration: ignore the \
                                 windows that look exactly like this one, and no others. {}",
                                ghost.platform.file(),
                                leaves_alone(&ghost),
                            ))
                            .small()
                            .color(theme::TEXT_DIM),
                        );
                        ui.horizontal(|ui| {
                            ui.label("application");
                            ui.add(
                                egui::TextEdit::singleline(&mut form.app).desired_width(f32::INFINITY),
                            );
                        });
                        let rule = form.rule();
                        let preview = rule.as_ref().map_or_else(|_| String::new(), Rule::preview);
                        if let Err(why) = &rule {
                            ui.label(
                                RichText::new(format!("No rule can be written: {why}"))
                                    .color(theme::BUG),
                            );
                        }
                        ui.add(
                            egui::TextEdit::multiline(&mut preview.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(12)
                                .desired_width(f32::INFINITY),
                        );

                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "Sending clones the repository into a temporary folder, commits the \
                                 rule on a new branch and opens a pull request on GitHub under your \
                                 account, with the credentials you already have set up for git (and \
                                 the GitHub CLI, if you have it). Nothing is sent until you press \
                                 the button.",
                            )
                            .small()
                            .color(theme::TEXT_DIM),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let rule = form.rule().ok();
                            if ui
                                .add_enabled(rule.is_some(), egui::Button::new("Send as a pull request"))
                                .clicked()
                            {
                                send = true;
                            }
                            if ui
                                .add_enabled(rule.is_some(), egui::Button::new("Copy the rule"))
                                .clicked()
                            {
                                copy = rule.map(|rule| rule.preview());
                            }
                        });
                    }
                    Stage::Sending { said, .. } => {
                        for step in said {
                            ui.label(RichText::new(format!("• {step}")).monospace().size(12.0));
                        }
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(
                                RichText::new("this can take a minute").color(theme::TEXT_DIM),
                            );
                        });
                    }
                    Stage::Done(outcome) => finished_ui(ui, outcome, &mut copy, &mut reveal),
                }
            });

        if let Some(path) = reveal {
            crate::platform::shell::reveal(&path);
        }
        if send {
            self.send_rule();
        }
        if let Some(text) = copy {
            ctx.copy_text(text);
            self.say("copied");
        }
        // a form that is being sent is not closed under the thread that is sending it
        if !open && !sending {
            self.rule = None;
        }
    }
}

/// What was seen of the ghost, in a sentence for the form.
fn what_was_seen(ghost: &GhostTile) -> String {
    let (from, to) = (
        timecode::format(ghost.first_video_s),
        timecode::format(ghost.last_video_s),
    );
    match ghost.platform {
        Platform::Windows => format!(
            "komorebi gave `{}` (class `{}`) a tile, and on {} reads between {from} and {to} Windows \
             said the window was {}. It was never on screen, so the tile is empty for as long as the \
             window exists.",
            ghost.exe, ghost.class, ghost.reads, ghost.hidden_how,
        ),
        Platform::Macos => format!(
            "komorebi gave `{}` ({}) a tile, and on {} reads between {from} and {to} the frame it \
             had for the window was {}, never one a person could see. The tile is empty for as long \
             as the window exists.",
            ghost.exe,
            if ghost.title.is_empty() {
                "no title".to_string()
            } else {
                format!("{:?}", ghost.title)
            },
            ghost.reads,
            ghost.hidden_how,
        ),
    }
}

/// What the rule leaves alone: the program's other windows that the recording saw.
fn leaves_alone(ghost: &GhostTile) -> String {
    match ghost.siblings.len() {
        0 => format!(
            "The recording saw no other window of {}, but there may be some, so the rule is \
             narrow all the same.",
            ghost.exe
        ),
        1 => format!(
            "{} has another window in the recording, which stays tiled.",
            ghost.exe
        ),
        many => format!(
            "{} has {many} other windows in the recording, which stay tiled.",
            ghost.exe
        ),
    }
}

/// What the form says when it is over.
fn finished_ui(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    copy: &mut Option<String>,
    reveal: &mut Option<std::path::PathBuf>,
) {
    match outcome {
        Outcome::AlreadyThere => {
            ui.label(
                RichText::new("The repository has this rule already, so there is nothing to send.")
                    .color(theme::OK),
            );
        }
        Outcome::Opened { url, branch } => {
            ui.label(RichText::new("The pull request is open.").color(theme::OK));
            ui.hyperlink(url);
            ui.label(
                RichText::new(format!("from the branch {branch}"))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
        Outcome::Pushed {
            branch,
            compare_url,
        } => {
            ui.label(
                RichText::new(format!(
                    "The branch {branch} is pushed. Open the pull request from it:"
                ))
                .color(theme::OK),
            );
            ui.hyperlink(compare_url);
        }
        Outcome::Failed(failure) => {
            ui.label(
                RichText::new(format!("Could not finish: {}", failure.step)).color(theme::BUG),
            );
            ui.label(RichText::new(&failure.error).monospace().size(11.0));
            ui.add_space(6.0);
            if let Some(dir) = &failure.dir {
                ui.horizontal(|ui| {
                    ui.label("The work is kept in");
                    ui.monospace(dir.display().to_string());
                });
                ui.horizontal(|ui| {
                    if ui.button("Open the folder").clicked() {
                        *reveal = Some(dir.clone());
                    }
                    if ui.button("Copy the path").clicked() {
                        *copy = Some(dir.display().to_string());
                    }
                });
            }
            ui.add_space(6.0);
            ui.label(RichText::new("To finish by hand").strong());
            ui.add(
                egui::TextEdit::multiline(&mut failure.instructions.as_str())
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Copy the instructions").clicked() {
                *copy = Some(failure.instructions.clone());
            }
        }
    }
}
