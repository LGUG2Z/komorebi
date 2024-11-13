use crate::render::RenderConfig;
use crate::ui::CustomUi;
use crate::widget::BarWidget;
use crate::MAX_LABEL_WIDTH;
use eframe::egui::text::LayoutJob;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::TextFormat;
use eframe::egui::TextStyle;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::sync::atomic::Ordering;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
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
        if let Ok(session) = self.session_manager.GetCurrentSession() {
            if let Ok(op) = session.TryTogglePlayPauseAsync() {
                op.get().unwrap_or_default();
            }
        }
    }

    fn output(&mut self) -> String {
        if let Ok(session) = self.session_manager.GetCurrentSession() {
            if let Ok(operation) = session.TryGetMediaPropertiesAsync() {
                if let Ok(properties) = operation.get() {
                    if let (Ok(artist), Ok(title)) = (properties.Artist(), properties.Title()) {
                        if artist.is_empty() {
                            return format!("{title}");
                        }

                        if title.is_empty() {
                            return format!("{artist}");
                        }

                        return format!("{artist} - {title}");
                    }
                }
            }
        }

        String::new()
    }
}

impl BarWidget for Media {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, mut config: RenderConfig) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let font_id = ctx
                    .style()
                    .text_styles
                    .get(&TextStyle::Body)
                    .cloned()
                    .unwrap_or_else(FontId::default);

                let mut layout_job = LayoutJob::simple(
                    egui_phosphor::regular::HEADPHONES.to_string(),
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                config.apply_on_widget(true, ui, |ui| {
                    let available_height = ui.available_height();
                    let mut custom_ui = CustomUi(ui);

                    if custom_ui
                        .add_sized_left_to_right(
                            Vec2::new(
                                MAX_LABEL_WIDTH.load(Ordering::SeqCst) as f32,
                                available_height,
                            ),
                            Label::new(layout_job)
                                .selectable(false)
                                .sense(Sense::click())
                                .truncate(),
                        )
                        .clicked()
                    {
                        self.toggle();
                    }
                });
            }
        }
    }
}
