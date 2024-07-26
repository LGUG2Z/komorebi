use crate::widget::BarWidget;
use sysinfo::Disks;

pub struct Storage {
    disks: Disks,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            disks: Disks::new_with_refreshed_list(),
        }
    }
}

impl BarWidget for Storage {
    fn output(&mut self) -> Vec<String> {
        self.disks.refresh();

        let mut disks = vec![];

        for disk in &self.disks {
            let mount = disk.mount_point();
            let total = disk.total_space();
            let available = disk.available_space();
            let used = total - available;

            disks.push(format!(
                "{} {}%",
                mount.to_string_lossy(),
                (used * 100) / total
            ))
        }

        disks.sort();
        disks.reverse();

        disks
    }
}
