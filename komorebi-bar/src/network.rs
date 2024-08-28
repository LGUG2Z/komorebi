use crate::widget::BarWidget;
use std::time::Duration;
use std::time::Instant;
use sysinfo::Networks;

#[derive(Copy, Clone, Debug)]
pub struct NetworkConfig {
    pub enable: bool,
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

impl BarWidget for Network {
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
