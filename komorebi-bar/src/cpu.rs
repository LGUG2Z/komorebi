use crate::bar::Alignment;
use crate::config::LabelPrefix;
use crate::widget::BarWidget;
use crate::widget::RenderConfig;
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
        let mut system =
            System::new_with_specifics(RefreshKind::default().without_memory().without_processes());

        system.refresh_cpu_usage();

        Self {
            enable: value.enable,
            system,
            data_refresh_interval: value.data_refresh_interval.unwrap_or(10),
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
            last_updated: Instant::now(),
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
    fn render(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        mut config: RenderConfig,
        alignment: Alignment,
    ) {
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
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::CPU.to_string()
                        }
                        LabelPrefix::None | LabelPrefix::Text => String::new(),
                    },
                    font_id.clone(),
                    ctx.style().visuals.selection.stroke.color,
                    100.0,
                );

                layout_job.append(
                    &output,
                    10.0,
                    TextFormat::simple(font_id, ctx.style().visuals.text_color()),
                );

                config.grouping.apply_on_widget(true, alignment, ui, |ui| {
                    if ui
                        .add(
                            Label::new(layout_job)
                                .selectable(false)
                                .sense(Sense::click()),
                        )
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
