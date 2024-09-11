use crate::widget::BarWidget;
use crate::WIDGET_SPACING;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Ui;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
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
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
}

impl From<NetworkConfig> for Network {
    fn from(value: NetworkConfig) -> Self {
        let mut last_state_data = vec![];
        let mut last_state_transmitted = vec![];

        let mut networks_total_data_transmitted = Networks::new_with_refreshed_list();
        let mut networks_network_activity = Networks::new_with_refreshed_list();

        let mut default_interface = String::new();

        if let Ok(interface) = netdev::get_default_interface() {
            if let Some(friendly_name) = interface.friendly_name {
                default_interface.clone_from(&friendly_name);

                if value.show_total_data_transmitted {
                    networks_total_data_transmitted.refresh();
                    for (interface_name, data) in &networks_total_data_transmitted {
                        if friendly_name.eq(interface_name) {
                            last_state_data.push(format!(
                                "{} {:.0} MB / {} {:.0} MB",
                                egui_phosphor::regular::ARROW_CIRCLE_DOWN,
                                (data.total_received() as f32) / 1024.0 / 1024.0,
                                egui_phosphor::regular::ARROW_CIRCLE_UP,
                                (data.total_transmitted() as f32) / 1024.0 / 1024.0,
                            ))
                        }
                    }
                }

                if value.show_network_activity {
                    networks_network_activity.refresh();
                    for (interface_name, data) in &networks_network_activity {
                        if friendly_name.eq(interface_name) {
                            last_state_transmitted.push(format!(
                                "{} {:.1} KB/s / {} {:.1} KB/s",
                                egui_phosphor::regular::ARROW_CIRCLE_DOWN,
                                (data.received() as f32) / 1024.0 / 1.0,
                                egui_phosphor::regular::ARROW_CIRCLE_UP,
                                (data.transmitted() as f32) / 1024.0 / 1.0,
                            ))
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
            show_total_data_transmitted: value.show_total_data_transmitted,
            show_network_activity: value.show_network_activity,
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
    default_interface: String,
    last_state_total_data_transmitted: Vec<String>,
    last_state_network_activity: Vec<String>,
    last_updated_total_data_transmitted: Instant,
    last_updated_network_activity: Instant,
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
                                outputs.push(format!(
                                    "{} {:.1} KB/s / {} {:.1} KB/s",
                                    egui_phosphor::regular::ARROW_CIRCLE_DOWN,
                                    (data.received() as f32)
                                        / 1024.0
                                        / self.data_refresh_interval as f32,
                                    egui_phosphor::regular::ARROW_CIRCLE_UP,
                                    (data.transmitted() as f32)
                                        / 1024.0
                                        / self.data_refresh_interval as f32,
                                ))
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
                                outputs.push(format!(
                                    "{} {:.0} MB / {} {:.0} MB",
                                    egui_phosphor::regular::ARROW_CIRCLE_DOWN,
                                    (data.total_received() as f32) / 1024.0 / 1024.0,
                                    egui_phosphor::regular::ARROW_CIRCLE_UP,
                                    (data.total_transmitted() as f32) / 1024.0 / 1024.0,
                                ))
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
    fn render(&mut self, _ctx: &Context, ui: &mut Ui) {
        if self.show_total_data_transmitted {
            for output in self.total_data_transmitted() {
                ui.add(Label::new(output).selectable(false));
            }

            ui.add_space(WIDGET_SPACING);
        }

        if self.show_network_activity {
            for output in self.network_activity() {
                ui.add(Label::new(output).selectable(false));
            }

            ui.add_space(WIDGET_SPACING);
        }

        if self.enable {
            self.default_interface();

            if !self.default_interface.is_empty()
                && ui
                    .add(
                        Label::new(format!(
                            "{} {}",
                            egui_phosphor::regular::WIFI_HIGH,
                            self.default_interface
                        ))
                        .selectable(false)
                        .sense(Sense::click()),
                    )
                    .clicked()
            {
                if let Err(error) = Command::new("cmd.exe").args(["/C", "ncpa"]).spawn() {
                    eprintln!("{}", error)
                }
            }

            ui.add_space(WIDGET_SPACING);
        }
    }
}
