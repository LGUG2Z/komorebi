use crate::MAX_LABEL_WIDTH;
use crate::bar::Alignment;
use crate::config::MediaDisplayFormat;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::ui::CustomUi;
use crate::widgets::widget::BarWidget;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use eframe::egui::text::LayoutJob;
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::Ordering;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
/// Media widget configuration
pub struct MediaConfig {
    /// Enable the Media widget
    pub enable: bool,
    /// Display format of the media widget (defaults to IconAndText)
    pub display: Option<MediaDisplayFormat>,
}

impl From<MediaConfig> for Media {
    fn from(value: MediaConfig) -> Self {
        Self::new(
            value.enable,
            value.display.unwrap_or(MediaDisplayFormat::IconAndText),
        )
    }
}

#[derive(Clone, Debug)]
pub struct Media {
    pub enable: bool,
    pub display: MediaDisplayFormat,
    pub session_manager: GlobalSystemMediaTransportControlsSessionManager,
}

impl Media {
    pub fn new(enable: bool, display: MediaDisplayFormat) -> Self {
        Self {
            enable,
            display,
            session_manager: GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .unwrap()
                .join()
                .unwrap(),
        }
    }

    pub fn toggle(&self) {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(op) = session.TryTogglePlayPauseAsync()
        {
            op.join().unwrap_or_default();
        }
    }

    pub fn previous(&self) {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(op) = session.TrySkipPreviousAsync()
        {
            op.join().unwrap_or_default();
        }
    }

    pub fn next(&self) {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(op) = session.TrySkipNextAsync()
        {
            op.join().unwrap_or_default();
        }
    }

    fn is_playing(&self) -> bool {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(info) = session.GetPlaybackInfo()
            && let Ok(status) = info.PlaybackStatus()
        {
            return status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        }
        false
    }

    fn is_previous_enabled(&self) -> bool {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(info) = session.GetPlaybackInfo()
            && let Ok(controls) = info.Controls()
            && let Ok(enabled) = controls.IsPreviousEnabled()
        {
            return enabled;
        }
        false
    }

    fn is_next_enabled(&self) -> bool {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(info) = session.GetPlaybackInfo()
            && let Ok(controls) = info.Controls()
            && let Ok(enabled) = controls.IsNextEnabled()
        {
            return enabled;
        }
        false
    }

    fn has_session(&self) -> bool {
        self.session_manager.GetCurrentSession().is_ok()
    }

    fn output(&mut self) -> String {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(operation) = session.TryGetMediaPropertiesAsync()
            && let Ok(properties) = operation.join()
            && let (Ok(artist), Ok(title)) = (properties.Artist(), properties.Title())
        {
            if artist.is_empty() {
                return format!("{title}");
            }

            if title.is_empty() {
                return format!("{artist}");
            }

            return format!("{artist} - {title}");
        }

        String::new()
    }
}

impl BarWidget for Media {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            // Don't render if there's no active media session
            if !self.has_session() {
                return;
            }

            let output = self.output();

            let show_icon = matches!(
                self.display,
                MediaDisplayFormat::Icon
                    | MediaDisplayFormat::IconAndText
                    | MediaDisplayFormat::IconAndControls
                    | MediaDisplayFormat::Full
            );
            let show_text = matches!(
                self.display,
                MediaDisplayFormat::Text
                    | MediaDisplayFormat::IconAndText
                    | MediaDisplayFormat::TextAndControls
                    | MediaDisplayFormat::Full
            );
            let show_controls = matches!(
                self.display,
                MediaDisplayFormat::ControlsOnly
                    | MediaDisplayFormat::IconAndControls
                    | MediaDisplayFormat::TextAndControls
                    | MediaDisplayFormat::Full
            );

            // Don't render if there's no media info and we're not showing controls-only
            if output.is_empty() && !show_controls {
                return;
            }

            let icon_font_id = config.icon_font_id.clone();
            let text_font_id = config.text_font_id.clone();
            let icon_color = ctx.style().visuals.selection.stroke.color;
            let text_color = ctx.style().visuals.text_color();

            let mut layout_job = LayoutJob::default();

            if show_icon {
                layout_job = LayoutJob::simple(
                    egui_phosphor::regular::HEADPHONES.to_string(),
                    icon_font_id.clone(),
                    icon_color,
                    100.0,
                );
            }

            if show_text {
                layout_job.append(
                    &output,
                    if show_icon { 10.0 } else { 0.0 },
                    TextFormat {
                        font_id: text_font_id,
                        color: text_color,
                        valign: Align::Center,
                        ..Default::default()
                    },
                );
            }

            let is_playing = self.is_playing();
            let is_previous_enabled = self.is_previous_enabled();
            let is_next_enabled = self.is_next_enabled();
            let disabled_color = text_color.gamma_multiply(0.5);
            let is_reversed = matches!(config.alignment, Some(Alignment::Right));

            let prev_color = if is_previous_enabled {
                text_color
            } else {
                disabled_color
            };

            let next_color = if is_next_enabled {
                text_color
            } else {
                disabled_color
            };

            let play_pause_icon = if is_playing {
                egui_phosphor::regular::PAUSE
            } else {
                egui_phosphor::regular::PLAY
            };

            let show_label = |ui: &mut Ui| {
                if (show_icon || show_text)
                    && SelectableFrame::new(false)
                        .show(ui, |ui| {
                            let available_height = ui.available_height();
                            let mut custom_ui = CustomUi(ui);

                            custom_ui.add_sized_left_to_right(
                                Vec2::new(
                                    MAX_LABEL_WIDTH.load(Ordering::SeqCst) as f32,
                                    available_height,
                                ),
                                Label::new(layout_job.clone()).selectable(false).truncate(),
                            )
                        })
                        .on_hover_text(&output)
                        .clicked()
                {
                    self.toggle();
                }
            };

            let show_previous = |ui: &mut Ui| {
                if SelectableFrame::new(false)
                    .show(ui, |ui| {
                        ui.add(
                            Label::new(LayoutJob::simple(
                                egui_phosphor::regular::SKIP_BACK.to_string(),
                                icon_font_id.clone(),
                                prev_color,
                                100.0,
                            ))
                            .selectable(false),
                        )
                    })
                    .clicked()
                    && is_previous_enabled
                {
                    self.previous();
                }
            };

            let show_play_pause = |ui: &mut Ui| {
                if SelectableFrame::new(false)
                    .show(ui, |ui| {
                        ui.add(
                            Label::new(LayoutJob::simple(
                                play_pause_icon.to_string(),
                                icon_font_id.clone(),
                                text_color,
                                100.0,
                            ))
                            .selectable(false),
                        )
                    })
                    .on_hover_text(&output)
                    .clicked()
                {
                    self.toggle();
                }
            };

            let show_next = |ui: &mut Ui| {
                if SelectableFrame::new(false)
                    .show(ui, |ui| {
                        ui.add(
                            Label::new(LayoutJob::simple(
                                egui_phosphor::regular::SKIP_FORWARD.to_string(),
                                icon_font_id.clone(),
                                next_color,
                                100.0,
                            ))
                            .selectable(false),
                        )
                    })
                    .clicked()
                    && is_next_enabled
                {
                    self.next();
                }
            };

            config.apply_on_widget(false, ui, |ui| {
                if is_reversed {
                    // Right panel renders right-to-left, so reverse order
                    if show_controls {
                        show_next(ui);
                        show_play_pause(ui);
                        show_previous(ui);
                    }
                    show_label(ui);
                } else {
                    // Left/center panel renders left-to-right, normal order
                    show_label(ui);
                    if show_controls {
                        show_previous(ui);
                        show_play_pause(ui);
                        show_next(ui);
                    }
                }
            });
        }
    }
}
