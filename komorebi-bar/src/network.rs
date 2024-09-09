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
    /// Show network transfer data
    pub show_data: bool,
}

impl From<NetworkConfig> for Network {
    fn from(value: NetworkConfig) -> Self {
        let mut last_state = vec![];
        let mut networks = Networks::new_with_refreshed_list();

        if let Ok(interface) = netdev::get_default_interface() {
            if let Some(friendly_name) = interface.friendly_name {
                last_state.push(friendly_name.clone());

                if value.show_data {
                    networks.refresh();
                    for (interface_name, data) in &networks {
                        if friendly_name.eq(interface_name) {
                            last_state.push(format!(
                                "{} MB (down) / {} MB (up)",
                                data.total_received() / 1024 / 1024,
                                data.total_transmitted() / 1024 / 1024,
                            ))
                        }
                    }
                }
            }
        }

        Self {
            enable: value.enable,
            last_state,
            networks,
            show_data: value.show_data,
            last_updated: Instant::now(),
        }
    }
}

pub struct Network {
    pub enable: bool,
    pub show_data: bool,
    networks: Networks,
    last_state: Vec<String>,
    last_updated: Instant,
}

impl Network {
    fn output(&mut self) -> Vec<String> {
        let mut outputs = self.last_state.clone();

        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(10) {
            outputs.clear();

            if let Ok(interface) = netdev::get_default_interface() {
                if let Some(friendly_name) = &interface.friendly_name {
                    outputs.push(friendly_name.clone());

                    if self.show_data {
                        self.networks.refresh();
                        for (interface_name, data) in &self.networks {
                            if friendly_name.eq(interface_name) {
                                outputs.push(format!(
                                    "{} MB (down) / {} MB (up)",
                                    data.total_received() / 1024 / 1024,
                                    data.total_transmitted() / 1024 / 1024,
                                ))
                            }
                        }
                    }
                }
            }

            self.last_state.clone_from(&outputs);
            self.last_updated = now;
        }

        outputs
    }
}

impl BarWidget for Network {
    fn render(&mut self, _ctx: &Context, ui: &mut Ui) {
        if self.enable {
            let output = self.output();

            if !output.is_empty() {
                match output.len() {
                    1 => {
                        if ui
                            .add(
                                Label::new(format!(
                                    "{} {}",
                                    egui_phosphor::regular::WIFI_HIGH,
                                    output[0]
                                ))
                                .selectable(false)
                                .sense(Sense::click()),
                            )
                            .clicked()
                        {
                            if let Err(error) = Command::new("cmd.exe").args(["/C", "ncpa"]).spawn()
                            {
                                eprintln!("{}", error)
                            }
                        }
                    }
                    2 => {
                        if ui
                            .add(
                                Label::new(format!(
                                    "{} {} - {}",
                                    egui_phosphor::regular::WIFI_HIGH,
                                    output[0],
                                    output[1]
                                ))
                                .selectable(false)
                                .sense(Sense::click()),
                            )
                            .clicked()
                        {
                            if let Err(error) = Command::new("cmd.exe").args(["/C", "ncpa"]).spawn()
                            {
                                eprintln!("{}", error)
                            }
                        };
                    }
                    _ => {}
                }

                ui.add_space(WIDGET_SPACING);
            }
        }
    }
}
