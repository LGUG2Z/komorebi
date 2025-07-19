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
use sysinfo::RefreshKind;
use sysinfo::System;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MemoryConfig {
    /// Enable the Memory widget
    pub enable: bool,
    /// Data refresh interval (default: 10 seconds)
    pub data_refresh_interval: Option<u64>,
    /// Display label prefix
    pub label_prefix: Option<LabelPrefix>,
    /// Select when the current percentage is over this value [[1-100]]
    pub auto_select_over: Option<u8>,
}

impl From<MemoryConfig> for Memory {
    fn from(value: MemoryConfig) -> Self {
        let data_refresh_interval = value.data_refresh_interval.unwrap_or(10);

        Self {
            enable: value.enable,
            system: System::new_with_specifics(
                RefreshKind::default().without_cpu().without_processes(),
            ),
            data_refresh_interval,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::IconAndText),
            auto_select_over: value.auto_select_over.map(|o| o.clamp(1, 100)),
            last_updated: Instant::now()
                .checked_sub(Duration::from_secs(data_refresh_interval))
                .unwrap(),
        }
    }
}

#[derive(Clone, Debug)]
struct MemoryOutput {
    label: String,
    selected: bool,
}

pub struct Memory {
    pub enable: bool,
    system: System,
    data_refresh_interval: u64,
    label_prefix: LabelPrefix,
    auto_select_over: Option<u8>,
    last_updated: Instant,
}

impl Memory {
    fn output(&mut self) -> MemoryOutput {
        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(self.data_refresh_interval) {
            self.system.refresh_memory();
            self.last_updated = now;
        }

        let used = self.system.used_memory();
        let total = self.system.total_memory();
        let usage = ((used * 100) / total) as u8;
        let selected = self.auto_select_over.is_some_and(|o| usage >= o);

        MemoryOutput {
            label: match self.label_prefix {
                LabelPrefix::Text | LabelPrefix::IconAndText => {
                    format!("RAM: {usage}%")
                }
                LabelPrefix::None | LabelPrefix::Icon => format!("{usage}%"),
            },
            selected,
        }
    }
}

impl BarWidget for Memory {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            let output = self.output();
            if !output.label.is_empty() {
                let auto_text_color = config.auto_select_text.filter(|_| output.selected);

                let mut layout_job = LayoutJob::simple(
                    match self.label_prefix {
                        LabelPrefix::Icon | LabelPrefix::IconAndText => {
                            egui_phosphor::regular::MEMORY.to_string()
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
                    {
                        if let Err(error) =
                            Command::new("cmd.exe").args(["/C", "taskmgr.exe"]).spawn()
                        {
                            eprintln!("{error}")
                        }
                    }
                });
            }
        }
    }
}
