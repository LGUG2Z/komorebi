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
use sysinfo::RefreshKind;
use sysinfo::System;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CpuConfig {
    /// Enable the Cpu widget
    pub enable: bool,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
}

impl From<CpuConfig> for Cpu {
    fn from(value: CpuConfig) -> Self {
        let data_refresh_interval = value.data_refresh_interval.unwrap_or(10);

        Self {
            enable: value.enable,
            system: System::new_with_specifics(
                RefreshKind::default().without_memory().without_processes(),
            ),
            data_refresh_interval,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
            last_updated: Instant::now()
                .checked_sub(Duration::from_secs(data_refresh_interval))
                .unwrap(),
        }
    }
}

pub struct Cpu {
    pub enable: bool,
    system: System,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    last_updated: Instant,
}

impl Cpu {
    fn output(&mut self) -> String {
        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            self.system.refresh_cpu_usage();
            self.last_updated = now;
        }

        let used = self.system.global_cpu_usage();
        match self.label_prefix {
            LabelPrefix::Text | LabelPrefix::IconAndText => format!("CPU: {:.0}%", used),
            LabelPrefix::None | LabelPrefix::Icon => format!("{:.0}%", used),
        }
    }
}

impl BarWidget for Cpu {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let output = self.output();
            if !output.is_empty() {
                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::CPU.to_string()
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
                        if let Err(error) =
                            Command::new("cmd.exe").args(["/C", "taskmgr.exe"]).spawn()
                        {
                            eprintln!("{}", error)
                        }
                    }
                });
            }
        }
    }
}
