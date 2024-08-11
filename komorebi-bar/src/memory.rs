use crate::widget::BarWidget;
use sysinfo::RefreshKind;
use sysinfo::System;

pub struct Memory {
    pub enable: bool,
    system: System,
}

#[derive(Copy, Clone, Debug)]
pub struct MemoryConfig {
    pub enable: bool,
}

impl From<MemoryConfig> for Memory {
    fn from(value: MemoryConfig) -> Self {
        Self {
            enable: value.enable,
            system: System::new_with_specifics(
                RefreshKind::default().without_cpu().without_processes(),
            ),
        }
    }
}

impl BarWidget for Memory {
    fn output(&mut self) -> Vec<String> {
        self.system.refresh_memory();
        let used = self.system.used_memory();
        let total = self.system.total_memory();
        vec![format!("RAM: {}%", (used * 100) / total)]
    }
}
