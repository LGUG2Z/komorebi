use crate::bar::Alignment;
use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::text::LayoutJob;
use serde::Deserialize;
use serde::Serialize;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use sysinfo::Disks;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StorageConfig {
    /// Enable the Storage widget
    pub enable: bool,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
    /// Show disks that are read only. (default: false)
    pub show_read_only_disks: Option<bool>,
    /// Show removable disks. (default: true)
    pub show_removable_disks: Option<bool>,
    /// Select when the current percentage is over this value [[1-100]]
    pub auto_select_over: Option<u8>,
    /// Hide when the current percentage is under this value [[1-100]]
    pub auto_hide_under: Option<u8>,
}

impl From<StorageConfig> for Storage {
    fn from(value: StorageConfig) -> Self {
        Self {
            enable: value.enable,
            disks: Disks::new_with_refreshed_list(),
            data_refresh_interval: value.data_refresh_interval.unwrap_or(10),
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
            show_read_only_disks: value.show_read_only_disks.unwrap_or(false),
            show_removable_disks: value.show_removable_disks.unwrap_or(true),
            auto_select_over: value.auto_select_over.map(|o| o.clamp(1, 100)),
            auto_hide_under: value.auto_hide_under.map(|o| o.clamp(1, 100)),
            last_updated: Instant::now(),
        }
    }
}

struct StorageDisk {
    label: String,
    selected: bool,
}

pub struct Storage {
    pub enable: bool,
    disks: Disks,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    show_read_only_disks: bool,
    show_removable_disks: bool,
    auto_select_over: Option<u8>,
    auto_hide_under: Option<u8>,
    last_updated: Instant,
}

impl Storage {
    fn output(&mut self) -> Vec<StorageDisk> {
        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            self.disks.refresh(true);
            self.last_updated = now;
        }

        let mut disks = vec![];

        for disk in &self.disks {
            if disk.is_read_only() && !self.show_read_only_disks {
                continue;
            }
            if disk.is_removable() && !self.show_removable_disks {
                continue;
            }
            let mount = disk.mount_point();
            let total = disk.total_space();
            let available = disk.available_space();
            let used = total - available;
            let percentage = ((used * 100) / total) as u8;

            let hide = self.auto_hide_under.is_some_and(|u| percentage <= u);

            if !hide {
                let selected = self.auto_select_over.is_some_and(|o| percentage >= o);

                disks.push(StorageDisk {
                    label: match self.label_prefix {
                        LabelPrefix::Text | LabelPrefix::IconAndText => {
                            format!("{} {}%", mount.to_string_lossy(), percentage)
                        }
                        LabelPrefix::None | LabelPrefix::Icon => format!("{percentage}%"),
                    },
                    selected,
                })
            }
        }

        disks.sort_by(|a, b| a.label.cmp(&b.label));

        disks
    }
}

impl BarWidget for Storage {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let mut output = self.output();
            let is_reversed = matches!(config.alignment, Some(Alignment::Right));

            if is_reversed {
                output.reverse();
            }

            for output in output {
                let auto_text_color = config.auto_select_text.filter(|_| output.selected);

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::HARD_DRIVES.to_string()
                        }
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
                        && let Err(error) = Command::new("cmd.exe")
                            .args([
                                "/C",
                                "explorer.exe",
                                output.label.split(' ').collect::<Vec<&str>>()[0],
                            ])
                            .spawn()
                    {
                        eprintln!("{error}")
                    }
                });
            }
        }
    }
}
