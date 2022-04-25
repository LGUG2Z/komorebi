use crate::widget::BarWidget;
use sysinfo::RefreshKind;
use sysinfo::System;
use sysinfo::SystemExt;

pub struct Ram;

impl BarWidget for Ram {
    fn output(&mut self) -> Vec<String> {
        let sys = System::new_with_specifics(RefreshKind::new().with_memory());
        let used = sys.used_memory();
        let total = sys.total_memory();
        vec![format!("RAM: {}%", (used * 100) / total)]
    }
}
