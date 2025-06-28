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
use serde::Deserialize;
use serde::Serialize;
use starship_battery::units::ratio::percent;
use starship_battery::Manager;
use starship_battery::State;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BatteryConfig {
    /// Enable the Battery widget
    pub enable: bool,
    /// Hide the widget if the battery is at full charge
    pub hide_on_full_charge: Option<bool>,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
    /// Select when the current percentage is under this value [[1-100]]
    pub auto_select_under: Option<u8>,
}

impl From<BatteryConfig> for Battery {
    fn from(value: BatteryConfig) -> Self {
        let data_refresh_interval = value.data_refresh_interval.unwrap_or(10);

        Self {
            enable: value.enable,
            hide_on_full_charge: value.hide_on_full_charge.unwrap_or(false),
            manager: Manager::new().unwrap(),
            last_state: None,
            data_refresh_interval,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
            auto_select_under: value.auto_select_under.map(|u| u.clamp(1, 100)),
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
    High,
    Medium,
    Low,
    Warning,
}

#[derive(Clone, Debug)]
struct BatteryOutput {
    label: String,
    selected: bool,
}

pub struct Battery {
    pub enable: bool,
    hide_on_full_charge: bool,
    manager: Manager,
    pub state: BatteryState,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    auto_select_under: Option<u8>,
    last_state: Option<BatteryOutput>,
    last_updated: Instant,
}

impl Battery {
    fn output(&mut self) -> Option<BatteryOutput> {
        let mut output = self.last_state.clone();

        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            output = None;

            if let Ok(mut batteries) = self.manager.batteries() {
                if let Some(Ok(first)) = batteries.nth(0) {
                    let percentage = first.state_of_charge().get::<percent>().round() as u8;

                    if percentage == 100 && self.hide_on_full_charge {
                        output = None
                    } else {
                        match first.state() {
                            State::Charging => self.state = BatteryState::Charging,
                            State::Discharging => {
                                self.state = match percentage {
                                    p if p > 75 => BatteryState::Discharging,
                                    p if p > 50 => BatteryState::High,
                                    p if p > 25 => BatteryState::Medium,
                                    p if p > 10 => BatteryState::Low,
                                    _ => BatteryState::Warning,
                                }
                            }
                            _ => {}
                        }

                        let selected = self.auto_select_under.is_some_and(|u| percentage <= u);

                        output = Some(BatteryOutput {
                            label: match self.label_prefix {
                                LabelPrefix::Text | LabelPrefix::IconAndText => {
                                    format!("BAT: {percentage}%")
                                }
                                LabelPrefix::None | LabelPrefix::Icon => {
                                    format!("{percentage}%")
                                }
                            },
                            selected,
                        })
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
            if let Some(output) = output {
                let emoji = match self.state {
                    BatteryState::Charging => egui_phosphor::regular::BATTERY_CHARGING,
                    BatteryState::Discharging => egui_phosphor::regular::BATTERY_FULL,
                    BatteryState::High => egui_phosphor::regular::BATTERY_HIGH,
                    BatteryState::Medium => egui_phosphor::regular::BATTERY_MEDIUM,
                    BatteryState::Low => egui_phosphor::regular::BATTERY_LOW,
                    BatteryState::Warning => egui_phosphor::regular::BATTERY_WARNING,
                };

                let auto_text_color = config.auto_select_text.filter(|_| output.selected);

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => emoji.to_string(),
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    config.icon_font_id.clone(),
                    auto_text_color.unwrap_or(ctx.style().visuals.selection.stroke.color),
                    100.0,
                );

                layout_job.append(
                    &output.label,
                    10.0,
                    TextFormat {
                        font_id: config.text_font_id.clone(),
                        color: auto_text_color.unwrap_or(ctx.style().visuals.text_color()),
                        valign: Align::Center,
                        ..Default::default()
                    },
                );

                let auto_focus_fill = config.auto_select_fill;

                config.apply_on_widget(false, ui, |ui| {
                    if SelectableFrame::new_auto(output.selected, auto_focus_fill)
                        .show(ui, |ui| ui.add(Label::new(layout_job).selectable(false)))
                        .clicked()
                    {
                        if let Err(error) = Command::new("cmd.exe")
                            .args(["/C", "start", "ms-settings:batterysaver"])
                            .spawn()
                        {
                            eprintln!("{error}")
                        }
                    }
                });
            }
        }
    }
}
