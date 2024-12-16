use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use sysinfo::Disks;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct StorageConfig {
    /// Enable the Storage widget
    pub enable: bool,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<StorageConfig> for Storage {
    fn from(value: StorageConfig) -> Self {
        Self {
            enable: value.enable,
            disks: Disks::new_with_refreshed_list(),
            data_refresh_interval: value.data_refresh_interval.unwrap_or(10),
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
            last_updated: Instant::now(),
        }
    }
}

pub struct Storage {
    pub enable: bool,
    disks: Disks,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    last_updated: Instant,
}

impl Storage {
    fn output(&mut self) -> Vec<String> {
        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            self.disks.refresh();
            self.last_updated = now;
        }

        let mut disks = vec![];

        for disk in &self.disks {
            let mount = disk.mount_point();
            let total = disk.total_space();
            let available = disk.available_space();
            let used = total - available;

            disks.push(match self.label_prefix {
                LabelPrefix::Text | LabelPrefix::IconAndText => {
                    format!("{} {}%", mount.to_string_lossy(), (used * 100) / total)
                }
                LabelPrefix::None | LabelPrefix::Icon => format!("{}%", (used * 100) / total),
            })
        }

        disks.sort();
        disks.reverse();

        disks
    }
}

impl BarWidget for Storage {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            for output in self.output() {
                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::HARD_DRIVES.to_string()
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
                        if let Err(error) = Command::new("cmd.exe")
                            .args([
                                "/C",
                                "explorer.exe",
                                output.split(' ').collect::<Vec<&str>>()[0],
                            ])
                            .spawn()
                        {
                            eprintln!("{}", error)
                        }
                    }
                });
            }
        }
    }
}
