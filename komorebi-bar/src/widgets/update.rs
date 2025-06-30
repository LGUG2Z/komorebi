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
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct UpdateConfig {
    /// Enable the Update widget
    pub enable: bool,
    /// Data refresh interval (default: 12 hours)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<UpdateConfig> for Update {
    fn from(value: UpdateConfig) -> Self {
        let data_refresh_interval = value.data_refresh_interval.unwrap_or(12);

        let mut latest_version = String::new();

        let client = reqwest::blocking::Client::new();
        if let Ok(response) = client
            .get("https://api.github.com/repos/LGUG2Z/komorebi/releases/latest")
            .header("User-Agent", "komorebi-bar-version-checker")
            .send()
        {
            #[derive(Deserialize)]
            struct Release {
                tag_name: String,
            }

            if let Ok(release) =
                serde_json::from_str::<Release>(&response.text().unwrap_or_default())
            {
                let trimmed = release.tag_name.trim_start_matches("v");
                latest_version = trimmed.to_string();
            }
        }

        Self {
            enable: value.enable,
            data_refresh_interval,
            installed_version: env!("CARGO_PKG_VERSION").to_string(),
            latest_version,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
            last_updated: Instant::now()
                .checked_sub(Duration::from_secs(data_refresh_interval))
                .unwrap(),
        }
    }
}

pub struct Update {
    pub enable: bool,
    data_refresh_interval: u64,
    installed_version: String,
    latest_version: String,
    label_prefix: LabelPrefix,
    last_updated: Instant,
}

impl Update {
    fn output(&mut self) -> String {
        let now = Instant::now();
        if now.duration_since(self.last_updated)
            > Duration::from_secs((self.data_refresh_interval * 60) * 60)
        {
            let client = reqwest::blocking::Client::new();
            if let Ok(response) = client
                .get("https://api.github.com/repos/LGUG2Z/komorebi/releases/latest")
                .header("User-Agent", "komorebi-bar-version-checker")
                .send()
            {
                #[derive(Deserialize)]
                struct Release {
                    tag_name: String,
                }

                if let Ok(release) =
                    serde_json::from_str::<Release>(&response.text().unwrap_or_default())
                {
                    let trimmed = release.tag_name.trim_start_matches("v");
                    self.latest_version = trimmed.to_string();
                }
            }

            self.last_updated = now;
        }

        if self.latest_version > self.installed_version {
            format!("Update available! v{}", self.latest_version)
        } else {
            String::new()
        }
    }
}

impl BarWidget for Update {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::ROCKET_LAUNCH.to_string()
                        }
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

                config.apply_on_widget(false, ui, |ui| {
                    if SelectableFrame::new(false)
                        .show(ui, |ui| ui.add(Label::new(layout_job).selectable(false)))
                        .clicked()
                    {
                        if let Err(error) = Command::new("explorer.exe")
                            .args([format!(
                                "https://github.com/LGUG2Z/komorebi/releases/v{}",
                                self.latest_version
                            )])
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
