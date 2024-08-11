use crate::widget::BarWidget;
use sysinfo::Disks;

#[derive(Copy, Clone, Debug)]
pub struct StorageConfig {
    pub enable: bool,
}

impl From<StorageConfig> for Storage {
    fn from(value: StorageConfig) -> Self {
        Self {
            enable: value.enable,
            disks: Disks::new_with_refreshed_list(),
        }
    }
}

pub struct Storage {
    pub enable: bool,
    disks: Disks,
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
