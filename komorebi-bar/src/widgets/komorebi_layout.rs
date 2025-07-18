use crate::config::DisplayFormat;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::komorebi::KomorebiLayoutConfig;
use eframe::egui::vec2;
use eframe::egui::Context;
use eframe::egui::CornerRadius;
use eframe::egui::FontId;
use eframe::egui::Frame;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::StrokeKind;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use komorebi_client::SocketMessage;
use serde::de::Error;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde_json::from_str;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Copy, Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum KomorebiLayout {
    Default(komorebi_client::DefaultLayout),
    Monocle,
    Floating,
    Paused,
    Custom,
}

impl<'de> Deserialize<'de> for KomorebiLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;

        // Attempt to deserialize the string as a DefaultLayout
        if let Ok(default_layout) = from_str::<komorebi_client::DefaultLayout>(&format!("\"{s}\""))
        {
            return Ok(KomorebiLayout::Default(default_layout));
        }

        // Handle other cases
        match s.as_str() {
            "Monocle" => Ok(KomorebiLayout::Monocle),
            "Floating" => Ok(KomorebiLayout::Floating),
            "Paused" => Ok(KomorebiLayout::Paused),
            "Custom" => Ok(KomorebiLayout::Custom),
            _ => Err(Error::custom(format!("Invalid layout: {s}"))),
        }
    }
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
                if komorebi_client::send_batch([
                    SocketMessage::FocusMonitorAtCursor,
                    SocketMessage::ToggleMonocle,
                ])
                .is_err()
                {
                    tracing::error!("could not send message to komorebi: ToggleMonocle");
                }
            }
            KomorebiLayout::Floating => {
                if komorebi_client::send_batch([
                    SocketMessage::FocusMonitorAtCursor,
                    SocketMessage::ToggleTiling,
                ])
                .is_err()
                {
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

    fn show_icon(&mut self, is_selected: bool, font_id: FontId, ctx: &Context, ui: &mut Ui) {
        // paint custom icons for the layout
        let size = Vec2::splat(font_id.size);
        let (response, painter) = ui.allocate_painter(size, Sense::hover());
        let color = if is_selected {
            ctx.style().visuals.selection.stroke.color
        } else {
            ui.style().visuals.text_color()
        };
        let stroke = Stroke::new(1.0, color);
        let mut rect = response.rect;
        let rounding = CornerRadius::same((rect.width() * 0.1) as u8);
        rect = rect.shrink(stroke.width);
        let c = rect.center();
        let r = rect.width() / 2.0;
        painter.rect_stroke(rect, rounding, stroke, StrokeKind::Outside);

        match self {
            KomorebiLayout::Default(layout) => match layout {
                komorebi_client::DefaultLayout::BSP => {
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c, c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c + vec2(r / 2.0, 0.0), c + vec2(r / 2.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::Columns => {
                    painter.line_segment([c - vec2(r / 2.0, r), c + vec2(-r / 2.0, r)], stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c - vec2(-r / 2.0, r), c + vec2(r / 2.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::Rows => {
                    painter.line_segment([c - vec2(r, r / 2.0), c + vec2(r, -r / 2.0)], stroke);
                    painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c - vec2(r, -r / 2.0), c + vec2(r, r / 2.0)], stroke);
                }
                komorebi_client::DefaultLayout::VerticalStack => {
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c, c + vec2(r, 0.0)], stroke);
                }
                komorebi_client::DefaultLayout::RightMainVerticalStack => {
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c - vec2(r, 0.0), c], stroke);
                }
                komorebi_client::DefaultLayout::HorizontalStack => {
                    painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c, c + vec2(0.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::UltrawideVerticalStack => {
                    painter.line_segment([c - vec2(r / 2.0, r), c + vec2(-r / 2.0, r)], stroke);
                    painter.line_segment([c + vec2(r / 2.0, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c - vec2(-r / 2.0, r), c + vec2(r / 2.0, r)], stroke);
                }
                komorebi_client::DefaultLayout::Grid => {
                    painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                }
                // TODO: @CtByte can you think of a nice icon to draw here?
                komorebi_client::DefaultLayout::Scrolling => {
                    painter.line_segment([c - vec2(r / 2.0, r), c + vec2(-r / 2.0, r)], stroke);
                    painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    painter.line_segment([c - vec2(-r / 2.0, r), c + vec2(r / 2.0, r)], stroke);
                }
            },
            KomorebiLayout::Monocle => {}
            KomorebiLayout::Floating => {
                let mut rect_left = response.rect;
                rect_left.set_width(rect.width() * 0.5);
                rect_left.set_height(rect.height() * 0.5);
                let mut rect_right = rect_left;
                rect_left = rect_left.translate(Vec2::new(
                    rect.width() * 0.1 + stroke.width,
                    rect.width() * 0.1 + stroke.width,
                ));
                rect_right = rect_right.translate(Vec2::new(
                    rect.width() * 0.35 + stroke.width,
                    rect.width() * 0.35 + stroke.width,
                ));
                painter.rect_filled(rect_left, rounding, color);
                painter.rect_stroke(rect_right, rounding, stroke, StrokeKind::Outside);
            }
            KomorebiLayout::Paused => {
                let mut rect_left = response.rect;
                rect_left.set_width(rect.width() * 0.25);
                rect_left.set_height(rect.height() * 0.8);
                let mut rect_right = rect_left;
                rect_left = rect_left.translate(Vec2::new(
                    rect.width() * 0.2 + stroke.width,
                    rect.width() * 0.1 + stroke.width,
                ));
                rect_right = rect_right.translate(Vec2::new(
                    rect.width() * 0.55 + stroke.width,
                    rect.width() * 0.1 + stroke.width,
                ));
                painter.rect_filled(rect_left, rounding, color);
                painter.rect_filled(rect_right, rounding, color);
            }
            KomorebiLayout::Custom => {
                painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                painter.line_segment([c + vec2(0.0, r / 2.0), c + vec2(r, r / 2.0)], stroke);
                painter.line_segment([c - vec2(0.0, r / 3.0), c - vec2(r, r / 3.0)], stroke);
            }
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
        let font_id = render_config.icon_font_id.clone();
        let mut show_options = RenderConfig::load_show_komorebi_layout_options();
        let format = layout_config.display.unwrap_or(DisplayFormat::IconAndText);

        if !self.is_default() {
            show_options = false;
        }

        render_config.apply_on_widget(false, ui, |ui| {
            let layout_frame = SelectableFrame::new(false)
                .show(ui, |ui| {
                    if let DisplayFormat::Icon | DisplayFormat::IconAndText = format {
                        self.show_icon(true, font_id.clone(), ctx, ui);
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
                    Frame::NONE.show(ui, |ui| {
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
                            //KomorebiLayout::Custom,
                            KomorebiLayout::Monocle,
                            KomorebiLayout::Floating,
                            KomorebiLayout::Paused,
                        ]);

                        for layout_option in &mut layout_options {
                            let is_selected = self == layout_option;

                            if SelectableFrame::new(is_selected)
                                .show(ui, |ui| {
                                    layout_option.show_icon(is_selected, font_id.clone(), ctx, ui)
                                })
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

        RenderConfig::store_show_komorebi_layout_options(show_options);
    }
}
