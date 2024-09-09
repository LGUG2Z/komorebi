use crate::widget::BarWidget;
use crate::WIDGET_SPACING;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Ui;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use starship_battery::units::ratio::percent;
use starship_battery::Manager;
use starship_battery::State;
use std::time::Duration;
use std::time::Instant;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BatteryConfig {
    /// Enable the Battery widget
    pub enable: bool,
}

impl From<BatteryConfig> for Battery {
    fn from(value: BatteryConfig) -> Self {
        let manager = Manager::new().unwrap();
        let mut last_state = vec![];
        let mut state = None;

        if let Ok(mut batteries) = manager.batteries() {
            if let Some(Ok(first)) = batteries.nth(0) {
                let percentage = first.state_of_charge().get::<percent>();
                match first.state() {
                    State::Charging => state = Some(BatteryState::Charging),
                    State::Discharging => state = Some(BatteryState::Discharging),
                    _ => {}
                }
                last_state.push(format!("{percentage}%"));
            }
        }

        Self {
            enable: value.enable,
            manager,
            last_state,
            state: state.unwrap_or(BatteryState::Discharging),
            last_updated: Instant::now(),
        }
    }
}

pub enum BatteryState {
    Charging,
    Discharging,
}

pub struct Battery {
    pub enable: bool,
    manager: Manager,
    pub state: BatteryState,
    last_state: Vec<String>,
    last_updated: Instant,
}

impl Battery {
    fn output(&mut self) -> Vec<String> {
        let mut outputs = self.last_state.clone();

        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(10) {
            outputs.clear();

            if let Ok(mut batteries) = self.manager.batteries() {
                if let Some(Ok(first)) = batteries.nth(0) {
                    let percentage = first.state_of_charge().get::<percent>();
                    match first.state() {
                        State::Charging => self.state = BatteryState::Charging,
                        State::Discharging => self.state = BatteryState::Discharging,
                        _ => {}
                    }

                    outputs.push(format!("{percentage}%"));
                }
            }

            self.last_state.clone_from(&outputs);
            self.last_updated = now;
        }

        outputs
    }
}

impl BarWidget for Battery {
    fn render(&mut self, _ctx: &Context, ui: &mut Ui) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                for battery in output {
                    let emoji = match self.state {
                        BatteryState::Charging => egui_phosphor::regular::BATTERY_CHARGING,
                        BatteryState::Discharging => egui_phosphor::regular::BATTERY_FULL,
                    };

                    ui.add(
                        Label::new(format!("{emoji} {battery}"))
                            .selectable(false)
                            .sense(Sense::click()),
                    );
                }

                ui.add_space(WIDGET_SPACING);
            }
        }
    }
}
