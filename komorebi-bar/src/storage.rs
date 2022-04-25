use crate::widget::BarWidget;
use crate::widget::Output;
use crate::widget::Widget;
use color_eyre::Result;
use sysinfo::DiskExt;
use sysinfo::RefreshKind;
use sysinfo::System;
use sysinfo::SystemExt;

pub struct Storage;

impl BarWidget for Storage {
    fn output(&mut self) -> Vec<String> {
        let sys = System::new_with_specifics(RefreshKind::new().with_disks_list());

        let mut disks = vec![];

        for disk in sys.disks() {
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

        disks.reverse();

        disks
    }
}
