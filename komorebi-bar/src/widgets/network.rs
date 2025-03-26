use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
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
    /// Show total data transmitted
    pub show_total_data_transmitted: bool,
    /// Show network activity
    pub show_network_activity: bool,
    /// Show default interface
    pub show_default_interface: Option<bool>,
    /// Characters to reserve for network activity data
    pub network_activity_fill_characters: Option<usize>,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<NetworkConfig> for Network {
    fn from(value: NetworkConfig) -> Self {
        let data_refresh_interval = value.data_refresh_interval.unwrap_or(10);

        Self {
            enable: value.enable,
            show_total_activity: value.show_total_data_transmitted,
            show_activity: value.show_network_activity,
            show_default_interface: value.show_default_interface.unwrap_or(true),
            networks_network_activity: Networks::new_with_refreshed_list(),
            default_interface: String::new(),
            data_refresh_interval,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
            network_activity_fill_characters: value.network_activity_fill_characters.unwrap_or_default(),
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
    default_interface: String,
    last_state_total_activity: Vec<NetworkReading>,
    last_state_activity: Vec<NetworkReading>,
    last_updated_network_activity: Instant,
    network_activity_fill_characters: usize,
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

        if now.duration_since(self.last_updated_network_activity) > Duration::from_secs(self.data_refresh_interval) {
            activity.clear();
            total_activity.clear();

            if let Ok(interface) = netdev::get_default_interface() {
                if let Some(friendly_name) = &interface.friendly_name {
                    self.default_interface.clone_from(friendly_name);

                    self.networks_network_activity.refresh(true);

                    for (interface_name, data) in &self.networks_network_activity {
                        if friendly_name.eq(interface_name) {
                            if self.show_activity {
                                activity.push(NetworkReading::new(
                                    NetworkReadingFormat::Speed,
                                    Self::to_pretty_bytes(data.received(), self.data_refresh_interval),
                                    Self::to_pretty_bytes(data.transmitted(), self.data_refresh_interval),
                                ));
                            }

                            if self.show_total_activity {
                                total_activity.push(NetworkReading::new(
                                    NetworkReadingFormat::Total,
                                    Self::to_pretty_bytes(data.total_received(), 1),
                                    Self::to_pretty_bytes(data.total_transmitted(), 1),
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

    fn reading_to_label(&self, ctx: &Context, reading: NetworkReading, config: RenderConfig) -> Label {
        let (text_down, text_up) = match self.label_prefix {
            LabelPrefix::None | LabelPrefix::Icon => match reading.format {
                NetworkReadingFormat::Speed => (
                    format!(
                        "{: >width$}/s ",
                        reading.received_text,
                        width = self.network_activity_fill_characters
                    ),
                    format!(
                        "{: >width$}/s",
                        reading.transmitted_text,
                        width = self.network_activity_fill_characters
                    ),
                ),
                NetworkReadingFormat::Total => (
                    format!("{} ", reading.received_text),
                    reading.transmitted_text,
                ),
            },
            LabelPrefix::Text | LabelPrefix::IconAndText => match reading.format {
                NetworkReadingFormat::Speed => (
                    format!(
                        "DOWN: {: >width$}/s ",
                        reading.received_text,
                        width = self.network_activity_fill_characters
                    ),
                    format!(
                        "UP: {: >width$}/s",
                        reading.transmitted_text,
                        width = self.network_activity_fill_characters
                    ),
                ),
                NetworkReadingFormat::Total => (
                    format!("\u{2211}DOWN: {}/s ", reading.received_text),
                    format!("\u{2211}UP: {}/s", reading.transmitted_text),
                ),
            },
        };

        let icon_format = TextFormat::simple(
            config.icon_font_id.clone(),
            ctx.style().visuals.selection.stroke.color,
        );
        let text_format = TextFormat {
            font_id: config.text_font_id.clone(),
            color: ctx.style().visuals.text_color(),
            valign: Align::Center,
            ..Default::default()
        };

        // icon
        let mut layout_job = LayoutJob::simple(
            match self.label_prefix {
                LabelPrefix::Icon | LabelPrefix::IconAndText => egui_phosphor::regular::ARROW_FAT_DOWN.to_string(),
                LabelPrefix::None | LabelPrefix::Text => String::new(),
            },
            icon_format.font_id.clone(),
            icon_format.color,
            100.0,
        );

        // text
        layout_job.append(
            &text_down,
            ctx.style().spacing.item_spacing.x,
            text_format.clone(),
        );

        // icon
        layout_job.append(
            &match self.label_prefix {
                LabelPrefix::Icon | LabelPrefix::IconAndText => egui_phosphor::regular::ARROW_FAT_UP.to_string(),
                LabelPrefix::None | LabelPrefix::Text => String::new(),
            },
            0.0,
            icon_format.clone(),
        );

        // text
        layout_job.append(
            &text_up,
            ctx.style().spacing.item_spacing.x,
            text_format.clone(),
        );

        Label::new(layout_job).selectable(false)
    }

    fn to_pretty_bytes(input_in_bytes: u64, timespan_in_s: u64) -> String {
        let input = input_in_bytes as f32 / timespan_in_s as f32;
        let mut magnitude = input.log(1024f32) as u32;

        // let the base unit be KiB
        if magnitude < 1 {
            magnitude = 1;
        }

        let base: Option<DataUnit> = num::FromPrimitive::from_u32(magnitude);
        let result = input / ((1u64) << (magnitude * 10)) as f32;

        match base {
            Some(DataUnit::B) => format!("{result:.1} B"),
            Some(unit) => format!("{result:.1} {unit}iB"),
            None => String::from("Unknown data unit"),
        }
    }
}

impl BarWidget for Network {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            // widget spacing: make sure to use the same config to call the apply_on_widget function
            let mut render_config = config.clone();

            if self.show_total_activity || self.show_activity {
                let (activity, total_activity) = self.network_activity();

                if self.show_total_activity {
                    for reading in total_activity {
                        render_config.apply_on_widget(true, ui, |ui| {
                            ui.add(self.reading_to_label(ctx, reading, config.clone()));
                        });
                    }
                }

                if self.show_activity {
                    for reading in activity {
                        render_config.apply_on_widget(true, ui, |ui| {
                            ui.add(self.reading_to_label(ctx, reading, config.clone()));
                        });
                    }
                }
            }

            if self.show_default_interface {
                self.default_interface();

                if !self.default_interface.is_empty() {
                    let mut layout_job = LayoutJob::simple(
                        match self.label_prefix {
                            LabelPrefix::Icon | LabelPrefix::IconAndText => egui_phosphor::regular::WIFI_HIGH.to_string(),
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
                        if SelectableFrame::new(false)
                            .show(ui, |ui| ui.add(Label::new(layout_job).selectable(false)))
                            .clicked()
                        {
                            if let Err(error) = Command::new("cmd.exe").args(["/C", "ncpa"]).spawn() {
                                eprintln!("{}", error)
                            }
                        }
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
struct NetworkReading {
    pub format: NetworkReadingFormat,
    pub received_text: String,
    pub transmitted_text: String,
}

impl NetworkReading {
    pub fn new(format: NetworkReadingFormat, received: String, transmitted: String) -> Self {
        NetworkReading {
            format,
            received_text: received,
            transmitted_text: transmitted,
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
        write!(f, "{:?}", self)
    }
}
