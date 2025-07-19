use crate::bar::Alignment;
use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Color32;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use num_derive::FromPrimitive;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use sysinfo::Networks;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct NetworkConfig {
    /// Enable the Network widget
    pub enable: bool,
    /// Show total received and transmitted activity
    #[serde(alias = "show_total_data_transmitted")]
    pub show_total_activity: bool,
    /// Show received and transmitted activity
    #[serde(alias = "show_network_activity")]
    pub show_activity: bool,
    /// Show default interface
    pub show_default_interface: Option<bool>,
    /// Characters to reserve for received and transmitted activity
    #[serde(alias = "network_activity_fill_characters")]
    pub activity_left_padding: Option<usize>,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
    /// Select when the value is over a limit (1MiB is 1048576 bytes (1024*1024))
    pub auto_select: Option<NetworkSelectConfig>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct NetworkSelectConfig {
    /// Select the total received data when it's over this value
    pub total_received_over: Option<u64>,
    /// Select the total transmitted data when it's over this value
    pub total_transmitted_over: Option<u64>,
    /// Select the received data when it's over this value
    pub received_over: Option<u64>,
    /// Select the transmitted data when it's over this value
    pub transmitted_over: Option<u64>,
}

impl From<NetworkConfig> for Network {
    fn from(value: NetworkConfig) -> Self {
        let data_refresh_interval = value.data_refresh_interval.unwrap_or(10);

        Self {
            enable: value.enable,
            show_total_activity: value.show_total_activity,
            show_activity: value.show_activity,
            show_default_interface: value.show_default_interface.unwrap_or(true),
            networks_network_activity: Networks::new_with_refreshed_list(),
            default_interface: String::new(),
            data_refresh_interval,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
            auto_select: value.auto_select,
            activity_left_padding: value.activity_left_padding.unwrap_or_default(),
            last_state_total_activity: vec![],
            last_state_activity: vec![],
            last_updated_network_activity: Instant::now()
                .checked_sub(Duration::from_secs(data_refresh_interval))
                .unwrap(),
        }
    }
}

pub struct Network {
    pub enable: bool,
    pub show_total_activity: bool,
    pub show_activity: bool,
    pub show_default_interface: bool,
    networks_network_activity: Networks,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    auto_select: Option<NetworkSelectConfig>,
    default_interface: String,
    last_state_total_activity: Vec<NetworkReading>,
    last_state_activity: Vec<NetworkReading>,
    last_updated_network_activity: Instant,
    activity_left_padding: usize,
}

impl Network {
    fn default_interface(&mut self) {
        if let Ok(interface) = netdev::get_default_interface() {
            if let Some(friendly_name) = &interface.friendly_name {
                self.default_interface.clone_from(friendly_name);
            }
        }
    }

    fn network_activity(&mut self) -> (Vec<NetworkReading>, Vec<NetworkReading>) {
        let mut activity = self.last_state_activity.clone();
        let mut total_activity = self.last_state_total_activity.clone();
        let now = Instant::now();

        if now.duration_since(self.last_updated_network_activity)
            > Duration::from_secs(self.data_refresh_interval)
        {
            activity.clear();
            total_activity.clear();

            if let Ok(interface) = netdev::get_default_interface() {
                if let Some(friendly_name) = &interface.friendly_name {
                    self.default_interface.clone_from(friendly_name);

                    self.networks_network_activity.refresh(true);

                    for (interface_name, data) in &self.networks_network_activity {
                        if friendly_name.eq(interface_name) {
                            if self.show_activity {
                                let received = Self::to_pretty_bytes(
                                    data.received(),
                                    self.data_refresh_interval,
                                );
                                let transmitted = Self::to_pretty_bytes(
                                    data.transmitted(),
                                    self.data_refresh_interval,
                                );

                                activity.push(NetworkReading::new(
                                    NetworkReadingFormat::Speed,
                                    ReadingValue::from(received),
                                    ReadingValue::from(transmitted),
                                ));
                            }

                            if self.show_total_activity {
                                let total_received =
                                    Self::to_pretty_bytes(data.total_received(), 1);
                                let total_transmitted =
                                    Self::to_pretty_bytes(data.total_transmitted(), 1);

                                total_activity.push(NetworkReading::new(
                                    NetworkReadingFormat::Total,
                                    ReadingValue::from(total_received),
                                    ReadingValue::from(total_transmitted),
                                ))
                            }
                        }
                    }
                }
            }

            self.last_state_activity.clone_from(&activity);
            self.last_state_total_activity.clone_from(&total_activity);
            self.last_updated_network_activity = now;
        }

        (activity, total_activity)
    }

    fn reading_to_labels(
        &self,
        select_received: bool,
        select_transmitted: bool,
        ctx: &Context,
        reading: &NetworkReading,
        config: RenderConfig,
    ) -> (Label, Label) {
        let (text_down, text_up) = match self.label_prefix {
            LabelPrefix::None | LabelPrefix::Icon => match reading.format {
                NetworkReadingFormat::Speed => (
                    format!(
                        "{: >width$}/s ",
                        reading.received.pretty,
                        width = self.activity_left_padding
                    ),
                    format!(
                        "{: >width$}/s",
                        reading.transmitted.pretty,
                        width = self.activity_left_padding
                    ),
                ),
                NetworkReadingFormat::Total => (
                    format!("{} ", reading.received.pretty),
                    reading.transmitted.pretty.clone(),
                ),
            },
            LabelPrefix::Text | LabelPrefix::IconAndText => match reading.format {
                NetworkReadingFormat::Speed => (
                    format!(
                        "DOWN: {: >width$}/s ",
                        reading.received.pretty,
                        width = self.activity_left_padding
                    ),
                    format!(
                        "UP: {: >width$}/s",
                        reading.transmitted.pretty,
                        width = self.activity_left_padding
                    ),
                ),
                NetworkReadingFormat::Total => (
                    format!("\u{2211}DOWN: {}/s ", reading.received.pretty),
                    format!("\u{2211}UP: {}/s", reading.transmitted.pretty),
                ),
            },
        };

        let auto_text_color_received = config.auto_select_text.filter(|_| select_received);
        let auto_text_color_transmitted = config.auto_select_text.filter(|_| select_transmitted);

        // icon
        let mut layout_job_down = LayoutJob::simple(
            match self.label_prefix {
                LabelPrefix::Icon | LabelPrefix::IconAndText => {
                    if select_received {
                        egui_phosphor::regular::ARROW_FAT_LINES_DOWN.to_string()
                    } else {
                        egui_phosphor::regular::ARROW_FAT_DOWN.to_string()
                    }
                }
                LabelPrefix::None | LabelPrefix::Text => String::new(),
            },
            config.icon_font_id.clone(),
            auto_text_color_received.unwrap_or(ctx.style().visuals.selection.stroke.color),
            100.0,
        );

        // text
        layout_job_down.append(
            &text_down,
            ctx.style().spacing.item_spacing.x,
            TextFormat {
                font_id: config.text_font_id.clone(),
                color: auto_text_color_received.unwrap_or(ctx.style().visuals.text_color()),
                valign: Align::Center,
                ..Default::default()
            },
        );

        // icon
        let mut layout_job_up = LayoutJob::simple(
            match self.label_prefix {
                LabelPrefix::Icon | LabelPrefix::IconAndText => {
                    if select_transmitted {
                        egui_phosphor::regular::ARROW_FAT_LINES_UP.to_string()
                    } else {
                        egui_phosphor::regular::ARROW_FAT_UP.to_string()
                    }
                }
                LabelPrefix::None | LabelPrefix::Text => String::new(),
            },
            config.icon_font_id.clone(),
            auto_text_color_transmitted.unwrap_or(ctx.style().visuals.selection.stroke.color),
            100.0,
        );

        // text
        layout_job_up.append(
            &text_up,
            ctx.style().spacing.item_spacing.x,
            TextFormat {
                font_id: config.text_font_id.clone(),
                color: auto_text_color_transmitted.unwrap_or(ctx.style().visuals.text_color()),
                valign: Align::Center,
                ..Default::default()
            },
        );

        (
            Label::new(layout_job_down).selectable(false),
            Label::new(layout_job_up).selectable(false),
        )
    }

    fn to_pretty_bytes(input_in_bytes: u64, timespan_in_s: u64) -> (u64, String) {
        let input = input_in_bytes as f32 / timespan_in_s as f32;
        let mut magnitude = input.log(1024f32) as u32;

        // let the base unit be KiB
        if magnitude < 1 {
            magnitude = 1;
        }

        let base: Option<DataUnit> = num::FromPrimitive::from_u32(magnitude);
        let result = input / ((1u64) << (magnitude * 10)) as f32;

        (
            input as u64,
            match base {
                Some(DataUnit::B) => format!("{result:.1} B"),
                Some(unit) => format!("{result:.1} {unit}iB"),
                None => String::from("Unknown data unit"),
            },
        )
    }

    fn show_frame<R>(
        &self,
        selected: bool,
        auto_focus_fill: Option<Color32>,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) {
        if SelectableFrame::new_auto(selected, auto_focus_fill)
            .show(ui, add_contents)
            .clicked()
        {
            if let Err(error) = Command::new("cmd.exe").args(["/C", "ncpa"]).spawn() {
                eprintln!("{error}");
            }
        }
    }
}

impl BarWidget for Network {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let is_reversed = matches!(config.alignment, Some(Alignment::Right));

            // widget spacing: make sure to use the same config to call the apply_on_widget function
            let mut render_config = config.clone();

            if self.show_total_activity || self.show_activity {
                let (activity, total_activity) = self.network_activity();

                if self.show_total_activity {
                    for reading in &total_activity {
                        render_config.apply_on_widget(false, ui, |ui| {
                            let select_received = self.auto_select.is_some_and(|f| {
                                f.total_received_over
                                    .is_some_and(|o| reading.received.value > o)
                            });
                            let select_transmitted = self.auto_select.is_some_and(|f| {
                                f.total_transmitted_over
                                    .is_some_and(|o| reading.transmitted.value > o)
                            });

                            let labels = self.reading_to_labels(
                                select_received,
                                select_transmitted,
                                ctx,
                                reading,
                                config.clone(),
                            );

                            if is_reversed {
                                self.show_frame(
                                    select_transmitted,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.1),
                                );
                                self.show_frame(
                                    select_received,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.0),
                                );
                            } else {
                                self.show_frame(
                                    select_received,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.0),
                                );
                                self.show_frame(
                                    select_transmitted,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.1),
                                );
                            }
                        });
                    }
                }

                if self.show_activity {
                    for reading in &activity {
                        render_config.apply_on_widget(false, ui, |ui| {
                            let select_received = self.auto_select.is_some_and(|f| {
                                f.received_over.is_some_and(|o| reading.received.value > o)
                            });
                            let select_transmitted = self.auto_select.is_some_and(|f| {
                                f.transmitted_over
                                    .is_some_and(|o| reading.transmitted.value > o)
                            });

                            let labels = self.reading_to_labels(
                                select_received,
                                select_transmitted,
                                ctx,
                                reading,
                                config.clone(),
                            );

                            if is_reversed {
                                self.show_frame(
                                    select_transmitted,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.1),
                                );
                                self.show_frame(
                                    select_received,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.0),
                                );
                            } else {
                                self.show_frame(
                                    select_received,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.0),
                                );
                                self.show_frame(
                                    select_transmitted,
                                    config.auto_select_fill,
                                    ui,
                                    |ui| ui.add(labels.1),
                                );
                            }
                        });
                    }
                }
            }

            if self.show_default_interface {
                self.default_interface();

                if !self.default_interface.is_empty() {
                    let mut layout_job = LayoutJob::simple(
                        match self.label_prefix {
                            LabelPrefix::Icon | LabelPrefix::IconAndText => {
                                egui_phosphor::regular::WIFI_HIGH.to_string()
                            }
                            LabelPrefix::None | LabelPrefix::Text => String::new(),
                        },
                        config.icon_font_id.clone(),
                        ctx.style().visuals.selection.stroke.color,
                        100.0,
                    );

                    if let LabelPrefix::Text | LabelPrefix::IconAndText = self.label_prefix {
                        self.default_interface.insert_str(0, "NET: ");
                    }

                    layout_job.append(
                        &self.default_interface,
                        10.0,
                        TextFormat {
                            font_id: config.text_font_id.clone(),
                            color: ctx.style().visuals.text_color(),
                            valign: Align::Center,
                            ..Default::default()
                        },
                    );

                    render_config.apply_on_widget(false, ui, |ui| {
                        self.show_frame(false, None, ui, |ui| {
                            ui.add(Label::new(layout_job).selectable(false))
                        });
                    });
                }
            }

            // widget spacing: pass on the config that was use for calling the apply_on_widget function
            *config = render_config.clone();
        }
    }
}

#[derive(Clone)]
enum NetworkReadingFormat {
    Speed = 0,
    Total = 1,
}

#[derive(Clone)]
struct ReadingValue {
    value: u64,
    pretty: String,
}

impl From<(u64, String)> for ReadingValue {
    fn from(value: (u64, String)) -> Self {
        Self {
            value: value.0,
            pretty: value.1,
        }
    }
}

#[derive(Clone)]
struct NetworkReading {
    format: NetworkReadingFormat,
    received: ReadingValue,
    transmitted: ReadingValue,
}

impl NetworkReading {
    fn new(
        format: NetworkReadingFormat,
        received: ReadingValue,
        transmitted: ReadingValue,
    ) -> Self {
        Self {
            format,
            received,
            transmitted,
        }
    }
}

#[derive(Debug, FromPrimitive)]
enum DataUnit {
    B = 0,
    K = 1,
    M = 2,
    G = 3,
    T = 4,
    P = 5,
    E = 6,
    Z = 7,
    Y = 8,
}

impl fmt::Display for DataUnit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
