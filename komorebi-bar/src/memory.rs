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
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use sysinfo::RefreshKind;
use sysinfo::System;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MemoryConfig {
    /// Enable the Memory widget
    pub enable: bool,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
}

impl From<MemoryConfig> for Memory {
    fn from(value: MemoryConfig) -> Self {
        let mut system =
            System::new_with_specifics(RefreshKind::default().without_cpu().without_processes());

        system.refresh_memory();

        Self {
            enable: value.enable,
            system,
            data_refresh_interval: value.data_refresh_interval.unwrap_or(10),
            last_updated: Instant::now(),
        }
    }
}

pub struct Memory {
    pub enable: bool,
    system: System,
    data_refresh_interval: u64,
    last_updated: Instant,
}

impl Memory {
    fn output(&mut self) -> String {
        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            self.system.refresh_memory();
            self.last_updated = now;
        }

        let used = self.system.used_memory();
        let total = self.system.total_memory();
        format!("RAM: {}%", (used * 100) / total)
    }
}

impl BarWidget for Memory {
    fn render(&mut self, ctx: &Context, ui: &mut Ui) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let font_id = ctx
                    .style()
                    .text_styles
                    .get(&TextStyle::Body)
                    .cloned()
                    .unwrap_or_else(FontId::default);

                let mut layout_job = LayoutJob::simple(
                    egui_phosphor::regular::MEMORY.to_string(),
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                if ui
                    .add(
                        Label::new(layout_job)
                            .selectable(false)
                            .sense(Sense::click()),
                    )
                    .clicked()
                {
                    if let Err(error) = Command::new("cmd.exe").args(["/C", "taskmgr.exe"]).spawn()
                    {
                        eprintln!("{}", error)
                    }
                }
            }

            ui.add_space(WIDGET_SPACING);
        }
    }
}
