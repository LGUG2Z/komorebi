use crate::bar::Alignment;
use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use chrono::Local;
use chrono::NaiveTime;
use chrono_tz::Tz;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::CornerRadius;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::Vec2;
use eframe::epaint::StrokeKind;
use lazy_static::lazy_static;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;
use std::time::Instant;

lazy_static! {
    static ref TIME_RANGES: Vec<(&'static str, NaiveTime)> = {
        vec![
            (
                egui_phosphor::regular::MOON,
                NaiveTime::from_hms_opt(0, 0, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::ALARM,
                NaiveTime::from_hms_opt(6, 0, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::BREAD,
                NaiveTime::from_hms_opt(6, 1, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::BARBELL,
                NaiveTime::from_hms_opt(6, 30, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::COFFEE,
                NaiveTime::from_hms_opt(8, 0, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::CLOCK,
                NaiveTime::from_hms_opt(8, 30, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::HAMBURGER,
                NaiveTime::from_hms_opt(12, 0, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::CLOCK_AFTERNOON,
                NaiveTime::from_hms_opt(12, 30, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::FORK_KNIFE,
                NaiveTime::from_hms_opt(18, 0, 0).expect("invalid"),
            ),
            (
                egui_phosphor::regular::MOON_STARS,
                NaiveTime::from_hms_opt(18, 30, 0).expect("invalid"),
            ),
        ]
    };
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TimeConfig {
    /// Enable the Time widget
    pub enable: bool,
    /// Set the Time format
    pub format: TimeFormat,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
    /// TimeZone (https://docs.rs/chrono-tz/latest/chrono_tz/enum.Tz.html)
    ///
    /// Use a custom format to display additional information, i.e.:
    /// ```json
    /// {
    ///     "Time": {
    ///         "enable": true,
    ///         "format": { "Custom": "%T %Z (Tokyo)" },
    ///         "timezone": "Asia/Tokyo"
    ///      }
    ///}
    /// ```
    pub timezone: Option<String>,
    /// Change the icon depending on the time. The default icon is used between 8:30 and 12:00. (default: false)
    pub changing_icon: Option<bool>,
}

impl From<TimeConfig> for Time {
    fn from(value: TimeConfig) -> Self {
        // using 1 second made the widget look "less accurate" and lagging (especially having multiple with seconds).
        // This is still better than getting an update every frame
        let data_refresh_interval = 500;

        Self {
            enable: value.enable,
            format: value.format,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
            timezone: value.timezone,
            changing_icon: value.changing_icon.unwrap_or_default(),
            data_refresh_interval_millis: data_refresh_interval,
            last_state: TimeOutput::new(),
            last_updated: Instant::now()
                .checked_sub(Duration::from_millis(data_refresh_interval))
                .unwrap(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
struct TimeOutput {
    label: String,
    icon: String,
}

impl TimeOutput {
    fn new() -> Self {
        Self {
            label: String::new(),
            icon: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Time {
    pub enable: bool,
    pub format: TimeFormat,
    label_prefix: LabelPrefix,
    timezone: Option<String>,
    changing_icon: bool,
    data_refresh_interval_millis: u64,
    last_state: TimeOutput,
    last_updated: Instant,
}

impl Time {
    fn output(&mut self) -> TimeOutput {
        let mut output = self.last_state.clone();
        let now = Instant::now();

        if now.duration_since(self.last_updated)
            > Duration::from_millis(self.data_refresh_interval_millis)
        {
            let (formatted, current_time) = match &self.timezone {
                Some(timezone) => match timezone.parse::<Tz>() {
                    Ok(tz) => {
                        let dt = Local::now().with_timezone(&tz);
                        (
                            dt.format(&self.format.fmt_string())
                                .to_string()
                                .trim()
                                .to_string(),
                            Some(dt.time()),
                        )
                    }
                    Err(_) => (format!("Invalid timezone: {timezone:?}"), None),
                },
                None => {
                    let dt = Local::now();
                    (
                        dt.format(&self.format.fmt_string())
                            .to_string()
                            .trim()
                            .to_string(),
                        Some(dt.time()),
                    )
                }
            };

            if current_time.is_none() {
                return TimeOutput {
                    label: formatted,
                    icon: egui_phosphor::regular::WARNING_CIRCLE.to_string(),
                };
            }

            let current_range = match &self.changing_icon {
                true => TIME_RANGES
                    .iter()
                    .rev()
                    .find(|&(_, start)| current_time.unwrap() > *start)
                    .cloned(),
                false => None,
            }
            .unwrap_or((egui_phosphor::regular::CLOCK, NaiveTime::default()));

            output = TimeOutput {
                label: formatted,
                icon: current_range.0.to_string(),
            };

            self.last_state.clone_from(&output);
            self.last_updated = now;
        }

        output
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
        let offset = height / 2.0 - height / 8.0;

        let (response, painter) =
            ui.allocate_painter(Vec2::new(width, full_height + offset * 2.0), Sense::hover());
        let color = ctx.style().visuals.text_color();

        let c = response.rect.center();
        let r = height / 2.0 - 0.5;

        if number == 1 || number == 3 || number == 5 || number == 7 || number == 9 {
            painter.circle_filled(c + Vec2::new(0.0, height * 1.50 + offset), r, color);
        } else {
            painter.circle_filled(c + Vec2::new(0.0, height * 1.50 + offset), r / 2.5, color);
        }

        if number == 2 || number == 3 || number == 6 || number == 7 {
            painter.circle_filled(c + Vec2::new(0.0, height * 0.50 + offset), r, color);
        } else {
            painter.circle_filled(c + Vec2::new(0.0, height * 0.50 + offset), r / 2.5, color);
        }

        if number == 4 || number == 5 || number == 6 || number == 7 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 0.50 + offset), r, color);
        } else if max_power > 2 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 0.50 + offset), r / 2.5, color);
        }

        if number == 8 || number == 9 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 1.50 + offset), r, color);
        } else if max_power > 3 {
            painter.circle_filled(c + Vec2::new(0.0, -height * 1.50 + offset), r / 2.5, color);
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
        let offset = height / 2.0 - height / 8.0;

        let (response, painter) =
            ui.allocate_painter(Vec2::new(width, full_height + offset * 2.0), Sense::hover());
        let color = ctx.style().visuals.text_color();
        let stroke = Stroke::new(1.0, color);

        let round_all = CornerRadius::same((response.rect.width() * 0.1) as u8);
        let round_top = CornerRadius {
            nw: round_all.nw,
            ne: round_all.ne,
            ..Default::default()
        };
        let round_none = CornerRadius::ZERO;
        let round_bottom = CornerRadius {
            sw: round_all.nw,
            se: round_all.ne,
            ..Default::default()
        };

        if max_power == 2 {
            let mut rect = response
                .rect
                .shrink2(Vec2::new(stroke.width, stroke.width + offset));
            rect.set_height(rect.height() - height * 2.0);
            rect = rect.translate(Vec2::new(0.0, height * 2.0 + offset));
            painter.rect_stroke(rect, round_all, stroke, StrokeKind::Outside);
        } else if max_power == 3 {
            let mut rect = response
                .rect
                .shrink2(Vec2::new(stroke.width, stroke.width + offset));
            rect.set_height(rect.height() - height);
            rect = rect.translate(Vec2::new(0.0, height + offset));
            painter.rect_stroke(rect, round_all, stroke, StrokeKind::Outside);
        } else {
            let mut rect = response
                .rect
                .shrink2(Vec2::new(stroke.width, stroke.width + offset));
            rect = rect.translate(Vec2::new(0.0, 0.0 + offset));
            painter.rect_stroke(rect, round_all, stroke, StrokeKind::Outside);
        }

        let mut rect_bin = response.rect;
        rect_bin.set_width(width);

        if number == 1 || number == 5 || number == 9 {
            rect_bin.set_height(height);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(stroke.width, height * 3.0 + offset * 2.0)),
                round_bottom,
                color,
            );
        }
        if number == 2 {
            rect_bin.set_height(height);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(stroke.width, height * 2.0 + offset * 2.0)),
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
                rect_bin.translate(Vec2::new(stroke.width, height * 2.0 + offset * 2.0)),
                round_bottom,
                color,
            );
        }
        if number == 4 || number == 5 {
            rect_bin.set_height(height);
            painter.rect_filled(
                rect_bin.translate(Vec2::new(stroke.width, height * 1.0 + offset * 2.0)),
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
                rect_bin.translate(Vec2::new(stroke.width, height * 1.0 + offset * 2.0)),
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
                rect_bin.translate(Vec2::new(stroke.width, height + offset * 2.0)),
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
            painter.rect_filled(
                rect_bin.translate(Vec2::new(stroke.width, 0.0 + offset * 2.0)),
                round_top,
                color,
            );
        }
    }
}

impl BarWidget for Time {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let mut output = self.output();
            if !output.label.is_empty() {
                let use_binary_circle = output.label.starts_with('c');
                let use_binary_rectangle = output.label.starts_with('r');

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => output.icon,
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    config.icon_font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                if let LabelPrefix::Text | LabelPrefix::IconAndText = self.label_prefix {
                    output.label.insert_str(0, "TIME: ");
                }

                if !use_binary_circle && !use_binary_rectangle {
                    layout_job.append(
                        &output.label,
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
                let is_reversed = matches!(config.alignment, Some(Alignment::Right));

                config.apply_on_widget(false, ui, |ui| {
                    if SelectableFrame::new(false)
                        .show(ui, |ui| {
                            if !is_reversed {
                                ui.add(Label::new(layout_job.clone()).selectable(false));
                            }

                            if use_binary_circle || use_binary_rectangle {
                                let ordered_output = if is_reversed {
                                    output.label.chars().rev().collect()
                                } else {
                                    output.label
                                };

                                for (section_index, section) in
                                    ordered_output.split(':').enumerate()
                                {
                                    ui.scope(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::splat(2.0);
                                        for (number_index, number_char) in
                                            section.chars().enumerate()
                                        {
                                            if let Some(number) = number_char.to_digit(10) {
                                                // the hour is the second char in the first section (in reverse, it's in the last section)
                                                let max_power = match (
                                                    is_reversed,
                                                    section_index,
                                                    number_index,
                                                ) {
                                                    (true, 2, 1) | (false, 0, 1) => 2,
                                                    (true, _, 1) | (false, _, 0) => 3,
                                                    _ => 4,
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

                            if is_reversed {
                                ui.add(Label::new(layout_job.clone()).selectable(false));
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
