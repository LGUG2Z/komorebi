use crate::bar::Alignment;
use crate::config::LabelPrefix;
use crate::widget::BarWidget;
use crate::widget::RenderConfig;
use eframe::egui::text::LayoutJob;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::TextFormat;
use eframe::egui::TextStyle;
use eframe::egui::Ui;
use num_derive::FromPrimitive;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use sysinfo::Networks;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct NetworkConfig {
    /// Enable the Network widget
    pub enable: bool,
    /// Show total data transmitted
    pub show_total_data_transmitted: bool,
    /// Show network activity
    pub show_network_activity: bool,
    /// Characters to reserve for network activity data
    pub network_activity_fill_characters: Option<usize>,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<NetworkConfig> for Network {
    fn from(value: NetworkConfig) -> Self {
        let mut last_state_data = vec![];
        let mut last_state_transmitted = vec![];

        let mut networks_total_data_transmitted = Networks::new_with_refreshed_list();
        let mut networks_network_activity = Networks::new_with_refreshed_list();

        let mut default_interface = String::new();

        let prefix = value.label_prefix.unwrap_or(LabelPrefix::Icon);

        if let Ok(interface) = netdev::get_default_interface() {
            if let Some(friendly_name) = interface.friendly_name {
                default_interface.clone_from(&friendly_name);

                if value.show_total_data_transmitted {
                    networks_total_data_transmitted.refresh();
                    for (interface_name, data) in &networks_total_data_transmitted {
                        if friendly_name.eq(interface_name) {
                            last_state_data.push(match prefix {
                                LabelPrefix::None => format!(
                                    "{} | {}",
                                    to_pretty_bytes(data.total_received(), 1),
                                    to_pretty_bytes(data.total_transmitted(), 1),
                                ),
                                LabelPrefix::Icon => format!(
                                    "{} {} | {} {}",
                                    egui_phosphor::regular::ARROW_FAT_DOWN,
                                    to_pretty_bytes(data.total_received(), 1),
                                    egui_phosphor::regular::ARROW_FAT_UP,
                                    to_pretty_bytes(data.total_transmitted(), 1),
                                ),
                                LabelPrefix::Text => format!(
                                    "\u{2211}DOWN: {} | \u{2211}UP: {}",
                                    to_pretty_bytes(data.total_received(), 1),
                                    to_pretty_bytes(data.total_transmitted(), 1),
                                ),
                                LabelPrefix::IconAndText => format!(
                                    "{} \u{2211}DOWN: {} | {} \u{2211}UP: {}",
                                    egui_phosphor::regular::ARROW_FAT_DOWN,
                                    to_pretty_bytes(data.total_received(), 1),
                                    egui_phosphor::regular::ARROW_FAT_UP,
                                    to_pretty_bytes(data.total_transmitted(), 1),
                                ),
                            })
                        }
                    }
                }

                if value.show_network_activity {
                    networks_network_activity.refresh();
                    for (interface_name, data) in &networks_network_activity {
                        if friendly_name.eq(interface_name) {
                            last_state_transmitted.push(match prefix {
                                LabelPrefix::None => format!(
                                    "{: >width$}/s | {: >width$}/s",
                                    to_pretty_bytes(data.received(), 1),
                                    to_pretty_bytes(data.transmitted(), 1),
                                    width =
                                        value.network_activity_fill_characters.unwrap_or_default(),
                                ),
                                LabelPrefix::Icon => format!(
                                    "{} {: >width$}/s | {} {: >width$}/s",
                                    egui_phosphor::regular::ARROW_FAT_DOWN,
                                    to_pretty_bytes(data.received(), 1),
                                    egui_phosphor::regular::ARROW_FAT_UP,
                                    to_pretty_bytes(data.transmitted(), 1),
                                    width =
                                        value.network_activity_fill_characters.unwrap_or_default(),
                                ),
                                LabelPrefix::Text => format!(
                                    "DOWN: {: >width$}/s | UP: {: >width$}/s",
                                    to_pretty_bytes(data.received(), 1),
                                    to_pretty_bytes(data.transmitted(), 1),
                                    width =
                                        value.network_activity_fill_characters.unwrap_or_default(),
                                ),
                                LabelPrefix::IconAndText => format!(
                                    "{} DOWN: {: >width$}/s | {} UP: {: >width$}/s",
                                    egui_phosphor::regular::ARROW_FAT_DOWN,
                                    to_pretty_bytes(data.received(), 1),
                                    egui_phosphor::regular::ARROW_FAT_UP,
                                    to_pretty_bytes(data.transmitted(), 1),
                                    width =
                                        value.network_activity_fill_characters.unwrap_or_default(),
                                ),
                            })
                        }
                    }
                }
            }
        }

        Self {
            enable: value.enable,
            networks_total_data_transmitted,
            networks_network_activity,
            default_interface,
            data_refresh_interval: value.data_refresh_interval.unwrap_or(10),
            label_prefix: prefix,
            show_total_data_transmitted: value.show_total_data_transmitted,
            show_network_activity: value.show_network_activity,
            network_activity_fill_characters: value
                .network_activity_fill_characters
                .unwrap_or_default(),
            last_state_total_data_transmitted: last_state_data,
            last_state_network_activity: last_state_transmitted,
            last_updated_total_data_transmitted: Instant::now(),
            last_updated_network_activity: Instant::now(),
        }
    }
}

pub struct Network {
    pub enable: bool,
    pub show_total_data_transmitted: bool,
    pub show_network_activity: bool,
    networks_total_data_transmitted: Networks,
    networks_network_activity: Networks,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    default_interface: String,
    last_state_total_data_transmitted: Vec<String>,
    last_state_network_activity: Vec<String>,
    last_updated_total_data_transmitted: Instant,
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

    fn network_activity(&mut self) -> Vec<String> {
        let mut outputs = self.last_state_network_activity.clone();
        let now = Instant::now();

        if self.show_network_activity
            && now.duration_since(self.last_updated_network_activity)
                > Duration::from_secs(self.data_refresh_interval)
        {
            outputs.clear();

            if let Ok(interface) = netdev::get_default_interface() {
                if let Some(friendly_name) = &interface.friendly_name {
                    if self.show_network_activity {
                        self.networks_network_activity.refresh();
                        for (interface_name, data) in &self.networks_network_activity {
                            if friendly_name.eq(interface_name) {
                                outputs.push(match self.label_prefix {
                                    LabelPrefix::None => format!(
                                        "{: >width$}/s | {: >width$}/s",
                                        to_pretty_bytes(
                                            data.received(),
                                            self.data_refresh_interval
                                        ),
                                        to_pretty_bytes(
                                            data.transmitted(),
                                            self.data_refresh_interval
                                        ),
                                        width = self.network_activity_fill_characters,
                                    ),
                                    LabelPrefix::Icon => format!(
                                        "{} {: >width$}/s | {} {: >width$}/s",
                                        egui_phosphor::regular::ARROW_FAT_DOWN,
                                        to_pretty_bytes(
                                            data.received(),
                                            self.data_refresh_interval
                                        ),
                                        egui_phosphor::regular::ARROW_FAT_UP,
                                        to_pretty_bytes(
                                            data.transmitted(),
                                            self.data_refresh_interval
                                        ),
                                        width = self.network_activity_fill_characters,
                                    ),
                                    LabelPrefix::Text => format!(
                                        "DOWN: {: >width$}/s | UP: {: >width$}/s",
                                        to_pretty_bytes(
                                            data.received(),
                                            self.data_refresh_interval
                                        ),
                                        to_pretty_bytes(
                                            data.transmitted(),
                                            self.data_refresh_interval
                                        ),
                                        width = self.network_activity_fill_characters,
                                    ),
                                    LabelPrefix::IconAndText => {
                                        format!(
                                            "{} DOWN: {: >width$}/s | {} UP: {: >width$}/s",
                                            egui_phosphor::regular::ARROW_FAT_DOWN,
                                            to_pretty_bytes(
                                                data.received(),
                                                self.data_refresh_interval
                                            ),
                                            egui_phosphor::regular::ARROW_FAT_UP,
                                            to_pretty_bytes(
                                                data.transmitted(),
                                                self.data_refresh_interval
                                            ),
                                            width = self.network_activity_fill_characters,
                                        )
                                    }
                                })
                            }
                        }
                    }
                }
            }

            self.last_state_network_activity.clone_from(&outputs);
            self.last_updated_network_activity = now;
        }

        outputs
    }

    fn total_data_transmitted(&mut self) -> Vec<String> {
        let mut outputs = self.last_state_total_data_transmitted.clone();
        let now = Instant::now();

        if self.show_total_data_transmitted
            && now.duration_since(self.last_updated_total_data_transmitted)
                > Duration::from_secs(self.data_refresh_interval)
        {
            outputs.clear();

            if let Ok(interface) = netdev::get_default_interface() {
                if let Some(friendly_name) = &interface.friendly_name {
                    if self.show_total_data_transmitted {
                        self.networks_total_data_transmitted.refresh();

                        for (interface_name, data) in &self.networks_total_data_transmitted {
                            if friendly_name.eq(interface_name) {
                                outputs.push(match self.label_prefix {
                                    LabelPrefix::None => format!(
                                        "{} | {}",
                                        to_pretty_bytes(data.total_received(), 1),
                                        to_pretty_bytes(data.total_transmitted(), 1),
                                    ),
                                    LabelPrefix::Icon => format!(
                                        "{} {} | {} {}",
                                        egui_phosphor::regular::ARROW_FAT_DOWN,
                                        to_pretty_bytes(data.total_received(), 1),
                                        egui_phosphor::regular::ARROW_FAT_UP,
                                        to_pretty_bytes(data.total_transmitted(), 1),
                                    ),
                                    LabelPrefix::Text => format!(
                                        "\u{2211}DOWN: {} | \u{2211}UP: {}",
                                        to_pretty_bytes(data.total_received(), 1),
                                        to_pretty_bytes(data.total_transmitted(), 1),
                                    ),
                                    LabelPrefix::IconAndText => format!(
                                        "{} \u{2211}DOWN: {} | {} \u{2211}UP: {}",
                                        egui_phosphor::regular::ARROW_FAT_DOWN,
                                        to_pretty_bytes(data.total_received(), 1),
                                        egui_phosphor::regular::ARROW_FAT_UP,
                                        to_pretty_bytes(data.total_transmitted(), 1),
                                    ),
                                })
                            }
                        }
                    }
                }
            }

            self.last_state_total_data_transmitted.clone_from(&outputs);
            self.last_updated_total_data_transmitted = now;
        }

        outputs
    }
}

impl BarWidget for Network {
    fn render(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        mut config: RenderConfig,
        alignment: Alignment,
    ) {
        if self.show_total_data_transmitted {
            for output in self.total_data_transmitted() {
                config.grouping.apply_on_widget(true, alignment, ui, |ui| {
                    ui.add(Label::new(output).selectable(false));
                });
            }
        }

        if self.show_network_activity {
            for output in self.network_activity() {
                config.grouping.apply_on_widget(true, alignment, ui, |ui| {
                    ui.add(Label::new(output).selectable(false));
                });
            }
        }

        if self.enable {
            self.default_interface();

            if !self.default_interface.is_empty() {
                let font_id = ctx
                    .style()
                    .text_styles
                    .get(&TextStyle::Body)
                    .cloned()
                    .unwrap_or_else(FontId::default);

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::WIFI_HIGH.to_string()
                        }
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                if let LabelPrefix::Text | LabelPrefix::IconAndText = self.label_prefix {
                    self.default_interface.insert_str(0, "NET: ");
                }

                layout_job.append(
                    &self.default_interface,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                config.grouping.apply_on_widget(true, alignment, ui, |ui| {
                    if ui
                        .add(
                            Label::new(layout_job)
                                .selectable(false)
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        if let Err(error) = Command::new("cmd.exe").args(["/C", "ncpa"]).spawn() {
                            eprintln!("{}", error)
                        }
                    }
                });
            }
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
