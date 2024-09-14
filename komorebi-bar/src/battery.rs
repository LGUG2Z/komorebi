use crate::widget::BarWidget;
use crate::WIDGET_SPACING;
use eframe::egui::text::LayoutJob;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::TextFormat;
use eframe::egui::TextStyle;
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
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
}

impl From<BatteryConfig> for Battery {
    fn from(value: BatteryConfig) -> Self {
        let manager = Manager::new().unwrap();
        let mut last_state = String::new();
        let mut state = None;

        if let Ok(mut batteries) = manager.batteries() {
            if let Some(Ok(first)) = batteries.nth(0) {
                let percentage = first.state_of_charge().get::<percent>();
                match first.state() {
                    State::Charging => state = Some(BatteryState::Charging),
                    State::Discharging => state = Some(BatteryState::Discharging),
                    _ => {}
                }

                last_state = format!("{percentage}%");
            }
        }

        Self {
            enable: value.enable,
            manager,
            last_state,
            data_refresh_interval: value.data_refresh_interval.unwrap_or(10),
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
    data_refresh_interval: u64,
    last_state: String,
    last_updated: Instant,
}

impl Battery {
    fn output(&mut self) -> String {
        let mut output = self.last_state.clone();

        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            output.clear();

            if let Ok(mut batteries) = self.manager.batteries() {
                if let Some(Ok(first)) = batteries.nth(0) {
                    let percentage = first.state_of_charge().get::<percent>();
                    match first.state() {
                        State::Charging => self.state = BatteryState::Charging,
                        State::Discharging => self.state = BatteryState::Discharging,
                        _ => {}
                    }

                    output = format!("{percentage}%");
                }
            }

            self.last_state.clone_from(&output);
            self.last_updated = now;
        }

        output
    }
}

impl BarWidget for Battery {
    fn render(&mut self, ctx: &Context, ui: &mut Ui) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let emoji = match self.state {
                    BatteryState::Charging => egui_phosphor::regular::BATTERY_CHARGING,
                    BatteryState::Discharging => egui_phosphor::regular::BATTERY_FULL,
                };

                let font_id = ctx
                    .style()
                    .text_styles
                    .get(&TextStyle::Body)
                    .cloned()
                    .unwrap_or_else(FontId::default);

                let mut layout_job = LayoutJob::simple(
                    emoji.to_string(),
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                ui.add(
                    Label::new(layout_job)
                        .selectable(false)
                        .sense(Sense::click()),
                );
            }

            ui.add_space(WIDGET_SPACING);
        }
    }
}
