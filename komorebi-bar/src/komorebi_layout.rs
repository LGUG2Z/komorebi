use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use eframe::egui::text::LayoutJob;
use eframe::egui::vec2;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Rounding;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use komorebi_client::SocketMessage;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum KomorebiLayout {
    Default(komorebi_client::DefaultLayout),
    //Monocle,
    Floating,
    Paused,
    Custom,
}

impl Display for KomorebiLayout {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            KomorebiLayout::Default(layout) => write!(f, "{layout}"),
            //KomorebiLayout::Monocle => write!(f, "Monocle"),
            KomorebiLayout::Floating => write!(f, "Floating"),
            KomorebiLayout::Paused => write!(f, "Paused"),
            KomorebiLayout::Custom => write!(f, "Custom"),
        }
    }
}

impl KomorebiLayout {
    fn show_icon(&mut self, font_id: FontId, ctx: &Context, ui: &mut Ui) -> Option<LayoutJob> {
        match self {
            KomorebiLayout::Default(layout) => {
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

                match layout {
                    komorebi_client::DefaultLayout::BSP => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                        painter.line_segment([c, c + vec2(r, 0.0)], stroke);
                        painter
                            .line_segment([c + vec2(r / 2.0, 0.0), c + vec2(r / 2.0, r)], stroke);
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
                }

                None
            }
            KomorebiLayout::Floating => Some(LayoutJob::simple(
                egui_phosphor::regular::BROWSERS.to_string(),
                font_id.clone(),
                ctx.style().visuals.selection.stroke.color,
                100.0,
            )),
            KomorebiLayout::Paused => Some(LayoutJob::simple(
                egui_phosphor::regular::PAUSE_CIRCLE.to_string(),
                font_id.clone(),
                ctx.style().visuals.selection.stroke.color,
                100.0,
            )),
            KomorebiLayout::Custom => Some(LayoutJob::simple(
                egui_phosphor::regular::USER_SQUARE.to_string(),
                font_id.clone(),
                ctx.style().visuals.selection.stroke.color,
                100.0,
            )),
        }
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        config: &mut RenderConfig,
        workspace_idx: Option<usize>,
        layouts: Option<Vec<KomorebiLayout>>,
    ) {
        let monitor_idx = config.monitor_idx;
        let font_id = ctx
            .style()
            .text_styles
            .get(&eframe::egui::TextStyle::Body)
            .cloned()
            .unwrap_or_else(eframe::egui::FontId::default);

        let mut show_options = config.states.show_komorebi_layout_options;

        config.apply_on_widget(false, ui, |ui| {
            let layout_frame = SelectableFrame::new(false)
                .show(ui, |ui| {
                    let widget = match self.show_icon(font_id.clone(), ctx, ui) {
                        Some(mut layout_job) => {
                            layout_job.append(
                                &self.to_string(),
                                ctx.style().spacing.item_spacing.x,
                                TextFormat::simple(
                                    font_id.clone(),
                                    ctx.style().visuals.text_color(),
                                ),
                            );

                            Label::new(layout_job)
                        }
                        None => Label::new(self.to_string()),
                    };

                    ui.add(widget.selectable(false))
                })
                .on_hover_text(self.to_string());

            if layout_frame.clicked() {
                show_options = !show_options;
            }

            if show_options {
                if let Some(workspace_idx) = workspace_idx {
                    eframe::egui::Frame::none().show(ui, |ui| {
                        ui.add(
                            Label::new(egui_phosphor::regular::ARROW_FAT_LINES_RIGHT.to_string())
                                .selectable(false),
                        );

                        let mut layout_options = match layouts {
                            Some(layouts) => layouts,
                            None => vec![
                                KomorebiLayout::Default(komorebi_client::DefaultLayout::BSP),
                                KomorebiLayout::Default(komorebi_client::DefaultLayout::Columns),
                                KomorebiLayout::Default(komorebi_client::DefaultLayout::Rows),
                                KomorebiLayout::Default(
                                    komorebi_client::DefaultLayout::VerticalStack,
                                ),
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
                                KomorebiLayout::Floating,
                                KomorebiLayout::Paused,
                            ],
                        };

                        for layout_option in &mut layout_options {
                            if SelectableFrame::new(false)
                                .show(ui, |ui| {
                                    if let Some(layout_job) =
                                        layout_option.show_icon(font_id.clone(), ctx, ui)
                                    {
                                        ui.add(Label::new(layout_job).selectable(false));
                                    }
                                })
                                .on_hover_text(match layout_option {
                                    KomorebiLayout::Default(layout) => layout.to_string(),
                                    KomorebiLayout::Floating => "Toggle tiling".to_string(),
                                    KomorebiLayout::Paused => "Toggle pause".to_string(),
                                    KomorebiLayout::Custom => "Custom".to_string(),
                                })
                                .clicked()
                            {
                                match layout_option {
                                    KomorebiLayout::Default(option) => {
                                        if komorebi_client::send_message(
                                            &SocketMessage::WorkspaceLayout(
                                                monitor_idx,
                                                workspace_idx,
                                                *option,
                                            ),
                                        )
                                        .is_err()
                                        {
                                            tracing::error!(
                                       F     "could not send message to komorebi: WorkspaceLayout"
                                        );
                                        }
                                    }
                                    KomorebiLayout::Floating => {
                                        if komorebi_client::send_message(
                                            &SocketMessage::ToggleTiling,
                                        )
                                        .is_err()
                                        {
                                            tracing::error!(
                                                "could not send message to komorebi: ToggleTiling"
                                            );
                                        }
                                    }
                                    KomorebiLayout::Paused => {
                                        if komorebi_client::send_message(
                                            &SocketMessage::TogglePause,
                                        )
                                        .is_err()
                                        {
                                            tracing::error!(
                                                "could not send message to komorebi: TogglePause"
                                            );
                                        }
                                    }
                                    KomorebiLayout::Custom => {}
                                }

                                show_options = false;
                            };
                        }
                    });
                }
            }
        });

        config.states.show_komorebi_layout_options = show_options;
    }
}
