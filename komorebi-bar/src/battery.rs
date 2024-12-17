use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::TextFormat;
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
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<BatteryConfig> for Battery {
    fn from(value: BatteryConfig) -> Self {
        let data_refresh_interval = value.data_refresh_interval.unwrap_or(10);

        Self {
            enable: value.enable,
            manager: Manager::new().unwrap(),
            last_state: String::new(),
            data_refresh_interval,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
            state: BatteryState::Discharging,
            last_updated: Instant::now()
                .checked_sub(Duration::from_secs(data_refresh_interval))
                .unwrap(),
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
    label_prefix: LabelPrefix,
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

                    output = match self.label_prefix {
                        LabelPrefix::Text | LabelPrefix::IconAndText => {
                            format!("BAT: {percentage:.0}%")
                        }
                        LabelPrefix::None | LabelPrefix::Icon => format!("{percentage:.0}%"),
                    }
                }
            }

            self.last_state.clone_from(&output);
            self.last_updated = now;
        }

        output
    }
}

impl BarWidget for Battery {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let emoji = match self.state {
                    BatteryState::Charging => egui_phosphor::regular::BATTERY_CHARGING,
                    BatteryState::Discharging => egui_phosphor::regular::BATTERY_FULL,
                };

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => emoji.to_string(),
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    config.icon_font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat {
                        font_id: config.text_font_id.clone(),
                        color: ctx.style().visuals.text_color(),
                        valign: Align::Center,
                        ..Default::default()
                    },
                );

                config.apply_on_widget(true, ui, |ui| {
                    ui.add(
                        Label::new(layout_job)
                            .selectable(false)
                            .sense(Sense::click()),
                    );
                });
            }
        }
    }
}
