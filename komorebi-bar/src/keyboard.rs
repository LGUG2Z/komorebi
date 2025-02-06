use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::WidgetText;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use windows::Globalization::Language;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KeyboardConfig {
    /// Enable the Input widget
    pub enable: bool,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<KeyboardConfig> for Keyboard {
    fn from(value: KeyboardConfig) -> Self {
        Self {
            enable: value.enable,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
        }
    }
}

pub struct Keyboard {
    pub enable: bool,
    pub label_prefix: LabelPrefix,
}

impl Keyboard {
    fn output(&mut self) -> String {
        let lang = Language::CurrentInputMethodLanguageTag()
            .map(|lang| lang.to_string())
            .unwrap_or_else(|_| "error".to_string());
        match self.label_prefix {
            LabelPrefix::Text | LabelPrefix::IconAndText => format!("KB: {}", lang),
            LabelPrefix::None | LabelPrefix::Icon => lang,
        }
    }
}

impl BarWidget for Keyboard {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::KEYBOARD.to_string()
                        }
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
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
                            ui.add(
                                Label::new(WidgetText::LayoutJob(layout_job.clone()))
                                    .selectable(false),
                            )
                        })
                        .clicked()
                    {}
                });
            }
        }
    }
}
