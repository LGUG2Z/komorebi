use crate::widget::BarWidget;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Ui;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use sysinfo::RefreshKind;
use sysinfo::System;

pub struct Memory {
    pub enable: bool,
    system: System,
    last_updated: Instant,
}

#[derive(Copy, Clone, Debug)]
pub struct MemoryConfig {
    pub enable: bool,
}

impl From<MemoryConfig> for Memory {
    fn from(value: MemoryConfig) -> Self {
        let mut system =
            System::new_with_specifics(RefreshKind::default().without_cpu().without_processes());

        system.refresh_memory();

        Self {
            enable: value.enable,
            system,
            last_updated: Instant::now(),
        }
    }
}

impl Memory {
    fn output(&mut self) -> Vec<String> {
        let now = Instant::now();
        if now.duration_since(self.last_updated) > Duration::from_secs(10) {
            self.system.refresh_memory();
            self.last_updated = now;
        }

        let used = self.system.used_memory();
        let total = self.system.total_memory();
        vec![format!("RAM: {}%", (used * 100) / total)]
    }
}

impl BarWidget for Memory {
    fn render(&mut self, _ctx: &Context, ui: &mut Ui) {
        if self.enable {
            for output in self.output() {
                if ui
                    .add(
                        Label::new(format!("🐏 {}", output))
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

            ui.add_space(10.0);
        }
    }
}
