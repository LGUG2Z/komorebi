use crate::widget::BarWidget;
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

impl BarWidget for Memory {
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
