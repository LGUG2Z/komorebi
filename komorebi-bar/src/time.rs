use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Rounding;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TimeConfig {
    /// Enable the Time widget
    pub enable: bool,
    /// Set the Time format
    pub format: TimeFormat,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<TimeConfig> for Time {
    fn from(value: TimeConfig) -> Self {
        Self {
            enable: value.enable,
            format: value.format,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum TimeFormat {
    /// Twelve-hour format (with seconds)
    TwelveHour,
    /// Twelve-hour format (without seconds)
    TwelveHourWithoutSeconds,
    /// Twenty-four-hour format (with seconds)
    TwentyFourHour,
    /// Twenty-four-hour format (without seconds)
    TwentyFourHourWithoutSeconds,
    /// Twenty-four-hour format displayed as a binary clock with circles (with seconds) (https://en.wikipedia.org/wiki/Binary_clock)
    BinaryCircle,
    /// Twenty-four-hour format displayed as a binary clock with rectangles (with seconds) (https://en.wikipedia.org/wiki/Binary_clock)
    BinaryRectangle,
    /// Custom format (https://docs.rs/chrono/latest/chrono/format/strftime/index.html)
    Custom(String),
}

impl TimeFormat {
    pub fn toggle(&mut self) {
        match self {
            TimeFormat::TwelveHour => *self = TimeFormat::TwelveHourWithoutSeconds,
            TimeFormat::TwelveHourWithoutSeconds => *self = TimeFormat::TwentyFourHour,
            TimeFormat::TwentyFourHour => *self = TimeFormat::TwentyFourHourWithoutSeconds,
            TimeFormat::TwentyFourHourWithoutSeconds => *self = TimeFormat::BinaryCircle,
            TimeFormat::BinaryCircle => *self = TimeFormat::BinaryRectangle,
            TimeFormat::BinaryRectangle => *self = TimeFormat::TwelveHour,
            _ => {}
        };
    }

    fn fmt_string(&self) -> String {
        match self {
            TimeFormat::TwelveHour => String::from("%l:%M:%S %p"),
            TimeFormat::TwelveHourWithoutSeconds => String::from("%l:%M %p"),
            TimeFormat::TwentyFourHour => String::from("%T"),
            TimeFormat::TwentyFourHourWithoutSeconds => String::from("%H:%M"),
            TimeFormat::BinaryCircle => String::from("c%T"),
            TimeFormat::BinaryRectangle => String::from("r%T"),
            TimeFormat::Custom(format) => format.to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Time {
    pub enable: bool,
    pub format: TimeFormat,
    label_prefix: LabelPrefix,
}

impl Time {
    fn output(&mut self) -> String {
        chrono::Local::now()
            .format(&self.format.fmt_string())
            .to_string()
            .trim()
            .to_string()
    }

    fn paint_binary_circle(
        &mut self,
        size: f32,
        number: u32,
        max_power: usize,
        ctx: &Context,
        ui: &mut Ui,
    ) {
        let full_height = size;
        let height = full_height / 4.0;
        let width = height;

        let (response, painter) =
            ui.allocate_painter(Vec2::new(width, full_height), Sense::hover());
        let color = ctx.style().visuals.text_color();

        let c = response.rect.center();
        let r = height / 2.0 - 0.5;

        if number == 1 || number == 3 || number == 5 || number == 7 || number == 9 {
            painter.circle_filled(c + Vec2::new(0.0, height * 1.50), r, color);
        } else {
            painter.circle_filled(c + Vec2::new(0.0, height * 1.50), r / 2.5, color);
        }

        if number == 2 || number == 3 || number == 6 || number == 7 {
            painter.circle_filled(c + Vec2::new(0.0, height * 0.50), r, color);
        } else {
            painter.circle_filled(c + Vec2::new(0.0, height * 0.50), r / 2.5, color);
        }

        if number == 4 || number == 5 || number == 6 || number == 7 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 0.50), r, color);
        } else if max_power > 2 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 0.50), r / 2.5, color);
        }

        if number == 8 || number == 9 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 1.50), r, color);
        } else if max_power > 3 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 1.50), r / 2.5, color);
        }
    }

    fn paint_binary_rect(
        &mut self,
        size: f32,
        number: u32,
        max_power: usize,
        ctx: &Context,
        ui: &mut Ui,
    ) {
        let full_height = size;
        let height = full_height / 4.0;
        let width = height * 1.5;

        let (response, painter) =
            ui.allocate_painter(Vec2::new(width, full_height), Sense::hover());
        let color = ctx.style().visuals.text_color();
        let stroke = Stroke::new(1.0, color);

        let round_all = Rounding::same(response.rect.width() * 0.1);
        let round_top = Rounding {
            nw: round_all.nw,
            ne: round_all.ne,
            ..Default::default()
        };
        let round_none = Rounding::ZERO;
        let round_bottom = Rounding {
            sw: round_all.nw,
            se: round_all.ne,
            ..Default::default()
        };

        if max_power == 2 {
            let mut rect = response.rect.shrink(stroke.width);
            rect.set_height(rect.height() - height * 2.0);
            rect = rect.translate(Vec2::new(0.0, height * 2.0));
            painter.rect_stroke(rect, round_all, stroke);
        } else if max_power == 3 {
            let mut rect = response.rect.shrink(stroke.width);
            rect.set_height(rect.height() - height);
            rect = rect.translate(Vec2::new(0.0, height));
            painter.rect_stroke(rect, round_all, stroke);
        } else {
            painter.rect_stroke(response.rect.shrink(stroke.width), round_all, stroke);
        }

        let mut rect_bin = response.rect;
        rect_bin.set_width(width);

        if number == 1 || number == 5 || number == 9 {
            rect_bin.set_height(height);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(0.0, height * 3.0)),
                round_bottom,
                color,
            );
        }
        if number == 2 {
            rect_bin.set_height(height);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(0.0, height * 2.0)),
                if max_power == 2 {
                    round_top
                } else {
                    round_none
                },
                color,
            );
        }
        if number == 3 {
            rect_bin.set_height(height * 2.0);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(0.0, height * 2.0)),
                round_bottom,
                color,
            );
        }
        if number == 4 || number == 5 {
            rect_bin.set_height(height);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(0.0, height * 1.0)),
                if max_power == 3 {
                    round_top
                } else {
                    round_none
                },
                color,
            );
        }
        if number == 6 {
            rect_bin.set_height(height * 2.0);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(0.0, height * 1.0)),
                if max_power == 3 {
                    round_top
                } else {
                    round_none
                },
                color,
            );
        }
        if number == 7 {
            rect_bin.set_height(height * 3.0);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(0.0, height)),
                if max_power == 3 {
                    round_all
                } else {
                    round_bottom
                },
                color,
            );
        }
        if number == 8 || number == 9 {
            rect_bin.set_height(height);
            painter.rect_filled(rect_bin.translate(Vec2::new(0.0, 0.0)), round_top, color);
        }
    }
}

impl BarWidget for Time {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let mut output = self.output();
            if !output.is_empty() {
                let use_binary_circle = output.starts_with('c');
                let use_binary_rectangle = output.starts_with('r');

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::CLOCK.to_string()
                        }
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    config.icon_font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                if let LabelPrefix::Text | LabelPrefix::IconAndText = self.label_prefix {
                    output.insert_str(0, "TIME: ");
                }

                if !use_binary_circle && !use_binary_rectangle {
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
                }

                let font_id = config.icon_font_id.clone();

                config.apply_on_widget(false, ui, |ui| {
                    if SelectableFrame::new(false)
                        .show(ui, |ui| {
                            ui.add(Label::new(layout_job).selectable(false));

                            if use_binary_circle || use_binary_rectangle {
                                for (section_index, section) in output.split(':').enumerate() {
                                    ui.scope(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::splat(2.0);
                                        for (number_index, number_char) in
                                            section.chars().enumerate()
                                        {
                                            if let Some(number) = number_char.to_digit(10) {
                                                // the hour is the second char in the first section
                                                let max_power =
                                                    if section_index == 0 && number_index == 1 {
                                                        2
                                                    } else if number_index == 0 {
                                                        3
                                                    } else {
                                                        4
                                                    };

                                                if use_binary_circle {
                                                    self.paint_binary_circle(
                                                        font_id.size,
                                                        number,
                                                        max_power,
                                                        ctx,
                                                        ui,
                                                    );
                                                } else if use_binary_rectangle {
                                                    self.paint_binary_rect(
                                                        font_id.size,
                                                        number,
                                                        max_power,
                                                        ctx,
                                                        ui,
                                                    );
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        })
                        .clicked()
                    {
                        self.format.toggle()
                    }
                });
            }
        }
    }
}
