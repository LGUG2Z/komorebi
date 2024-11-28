use crate::config::DisplayFormat;
use crate::komorebi::KomorebiLayoutConfig;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use eframe::egui::vec2;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Rounding;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use komorebi_client::SocketMessage;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
//#[serde(untagged)]
pub enum KomorebiLayout {
    Default(komorebi_client::DefaultLayout),
    Monocle,
    Floating,
    Paused,
    Custom,
}

impl Display for KomorebiLayout {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KomorebiLayout::Default(layout) => write!(f, "{layout}"),
            KomorebiLayout::Monocle => write!(f, "Monocle"),
            KomorebiLayout::Floating => write!(f, "Floating"),
            KomorebiLayout::Paused => write!(f, "Paused"),
            KomorebiLayout::Custom => write!(f, "Custom"),
        }
    }
}

impl KomorebiLayout {
    fn is_default(&mut self) -> bool {
        matches!(self, KomorebiLayout::Default(_))
    }

    fn on_click(
        &mut self,
        show_options: &bool,
        monitor_idx: usize,
        workspace_idx: Option<usize>,
    ) -> bool {
        if self.is_default() {
            !show_options
        } else {
            self.on_click_option(monitor_idx, workspace_idx);
            false
        }
    }

    fn on_click_option(&mut self, monitor_idx: usize, workspace_idx: Option<usize>) {
        match self {
            KomorebiLayout::Default(option) => {
                if let Some(ws_idx) = workspace_idx {
                    if komorebi_client::send_message(&SocketMessage::WorkspaceLayout(
                        monitor_idx,
                        ws_idx,
                        *option,
                    ))
                    .is_err()
                    {
                        tracing::error!("could not send message to komorebi: WorkspaceLayout");
                    }
                }
            }
            KomorebiLayout::Monocle => {
                if komorebi_client::send_message(&SocketMessage::ToggleMonocle).is_err() {
                    tracing::error!("could not send message to komorebi: ToggleMonocle");
                }
            }
            KomorebiLayout::Floating => {
                if komorebi_client::send_message(&SocketMessage::ToggleTiling).is_err() {
                    tracing::error!("could not send message to komorebi: ToggleTiling");
                }
            }
            KomorebiLayout::Paused => {
                if komorebi_client::send_message(&SocketMessage::TogglePause).is_err() {
                    tracing::error!("could not send message to komorebi: TogglePause");
                }
            }
            KomorebiLayout::Custom => {}
        }
    }

    fn show_icon(&mut self, font_id: FontId, ctx: &Context, ui: &mut Ui) {
        let rounding = Rounding::same(1.0);

        // paint custom icons for the layout
        let size = Vec2::splat(font_id.size);
        let (response, painter) = ui.allocate_painter(size, Sense::hover());
        let color = ctx.style().visuals.selection.stroke.color;
        let stroke = Stroke::new(1.0, color);
        let mut rect = response.rect;
        rect = rect.shrink(stroke.width);
        let c = rect.center();
        let r = rect.width() / 2.0;

        match self {
            KomorebiLayout::Default(layout) => match layout {
                komorebi_client::DefaultLayout::BSP => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c, c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c + vec2(r / 2.0, 0.0), c + vec2(r / 2.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::Columns => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(r / 2.0, r), c + vec2(-r / 2.0, r)], stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c - vec2(-r / 2.0, r), c + vec2(r / 2.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::Rows => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(r, r / 2.0), c + vec2(r, -r / 2.0)], stroke);
                    painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c - vec2(r, -r / 2.0), c + vec2(r, r / 2.0)], stroke);
                }
                komorebi_client::DefaultLayout::VerticalStack => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c, c + vec2(r, 0.0)], stroke);
                }
                komorebi_client::DefaultLayout::RightMainVerticalStack => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c - vec2(r, 0.0), c], stroke);
                }
                komorebi_client::DefaultLayout::HorizontalStack => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c, c + vec2(0.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::UltrawideVerticalStack => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(r / 2.0, r), c + vec2(-r / 2.0, r)], stroke);
                    painter.line_segment([c + vec2(r / 2.0, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c - vec2(-r / 2.0, r), c + vec2(r / 2.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::Grid => {
                    painter.rect_stroke(rect, rounding, stroke);
                    painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                }
            },
            KomorebiLayout::Monocle => {
                painter.rect_stroke(response.rect.shrink(stroke.width), rounding, stroke);
            }
            KomorebiLayout::Floating => {
                painter.rect_stroke(response.rect.shrink(stroke.width), rounding, stroke);
                // TODO
            }
            KomorebiLayout::Paused => {
                painter.rect_stroke(response.rect.shrink(stroke.width), rounding, stroke);
                // TODO
            }
            KomorebiLayout::Custom => {}
        }
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        render_config: &mut RenderConfig,
        layout_config: &KomorebiLayoutConfig,
        workspace_idx: Option<usize>,
    ) {
        let monitor_idx = render_config.monitor_idx;
        let font_id = ctx
            .style()
            .text_styles
            .get(&eframe::egui::TextStyle::Body)
            .cloned()
            .unwrap_or_else(eframe::egui::FontId::default);

        let mut show_options = render_config.states.show_komorebi_layout_options;
        let format = layout_config.display.unwrap_or(DisplayFormat::Icon);

        if !self.is_default() {
            show_options = false;
        }

        render_config.apply_on_widget(false, ui, |ui| {
            let layout_frame = SelectableFrame::new(false)
                .show(ui, |ui| {
                    if let DisplayFormat::Icon | DisplayFormat::IconAndText = format {
                        self.show_icon(font_id.clone(), ctx, ui);
                    }

                    if let DisplayFormat::Text | DisplayFormat::IconAndText = format {
                        ui.add(Label::new(self.to_string()).selectable(false));
                    }
                })
                .on_hover_text(self.to_string());

            if layout_frame.clicked() {
                show_options = self.on_click(&show_options, monitor_idx, workspace_idx);
            }

            if show_options {
                if let Some(workspace_idx) = workspace_idx {
                    eframe::egui::Frame::none().show(ui, |ui| {
                        ui.add(
                            Label::new(egui_phosphor::regular::ARROW_FAT_LINES_RIGHT.to_string())
                                .selectable(false),
                        );

                        let mut layout_options = layout_config.options.clone().unwrap_or(vec![
                            KomorebiLayout::Default(komorebi_client::DefaultLayout::BSP),
                            KomorebiLayout::Default(komorebi_client::DefaultLayout::Columns),
                            KomorebiLayout::Default(komorebi_client::DefaultLayout::Rows),
                            KomorebiLayout::Default(komorebi_client::DefaultLayout::VerticalStack),
                            KomorebiLayout::Default(
                                komorebi_client::DefaultLayout::RightMainVerticalStack,
                            ),
                            KomorebiLayout::Default(
                                komorebi_client::DefaultLayout::HorizontalStack,
                            ),
                            KomorebiLayout::Default(
                                komorebi_client::DefaultLayout::UltrawideVerticalStack,
                            ),
                            KomorebiLayout::Default(komorebi_client::DefaultLayout::Grid),
                            KomorebiLayout::Monocle,
                            KomorebiLayout::Floating,
                            KomorebiLayout::Paused,
                        ]);

                        for layout_option in &mut layout_options {
                            if SelectableFrame::new(self == layout_option)
                                .show(ui, |ui| layout_option.show_icon(font_id.clone(), ctx, ui))
                                .on_hover_text(match layout_option {
                                    KomorebiLayout::Default(layout) => layout.to_string(),
                                    KomorebiLayout::Monocle => "Toggle monocle".to_string(),
                                    KomorebiLayout::Floating => "Toggle tiling".to_string(),
                                    KomorebiLayout::Paused => "Toggle pause".to_string(),
                                    KomorebiLayout::Custom => "Custom".to_string(),
                                })
                                .clicked()
                            {
                                layout_option.on_click_option(monitor_idx, Some(workspace_idx));
                                show_options = false;
                            };
                        }
                    });
                }
            }
        });

        render_config.states.show_komorebi_layout_options = show_options;
    }
}
