use crate::widget::BarWidget;
use starship_battery::units::ratio::percent;
use starship_battery::Manager;
use starship_battery::State;
use std::time::Duration;
use std::time::Instant;

#[derive(Copy, Clone, Debug)]
pub struct BatteryConfig {
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

impl BarWidget for Battery {
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
