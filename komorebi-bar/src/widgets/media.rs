use crate::MAX_LABEL_WIDTH;
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

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MediaConfig {
    /// Enable the Media widget
    pub enable: bool,
}

impl From<MediaConfig> for Media {
    fn from(value: MediaConfig) -> Self {
        Self::new(value.enable)
    }
}

#[derive(Clone, Debug)]
pub struct Media {
    pub enable: bool,
    pub session_manager: GlobalSystemMediaTransportControlsSessionManager,
}

impl Media {
    pub fn new(enable: bool) -> Self {
        Self {
            enable,
            session_manager: GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .unwrap()
                .get()
                .unwrap(),
        }
    }

    pub fn toggle(&self) {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(op) = session.TryTogglePlayPauseAsync()
        {
            op.get().unwrap_or_default();
        }
    }

    fn output(&mut self) -> String {
        if let Ok(session) = self.session_manager.GetCurrentSession()
            && let Ok(operation) = session.TryGetMediaPropertiesAsync()
            && let Ok(properties) = operation.get()
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
            let output = self.output();
            if !output.is_empty() {
                let mut layout_job = LayoutJob::simple(
                    egui_phosphor::regular::HEADPHONES.to_string(),
                    config.icon_font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat {
                        font_id: config.text_font_id.clone(),
                        color: ctx.style().visuals.text_color(),
                        valign: Align::Center,
                        ..Default::default()
                    },
                );

                config.apply_on_widget(false, ui, |ui| {
                    if SelectableFrame::new(false)
                        .show(ui, |ui| {
                            let available_height = ui.available_height();
                            let mut custom_ui = CustomUi(ui);

                            custom_ui.add_sized_left_to_right(
                                Vec2::new(
                                    MAX_LABEL_WIDTH.load(Ordering::SeqCst) as f32,
                                    available_height,
                                ),
                                Label::new(layout_job).selectable(false).truncate(),
                            )
                        })
                        .clicked()
                    {
                        self.toggle();
                    }
                });
            }
        }
    }
}
