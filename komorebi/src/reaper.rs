#![deny(clippy::unwrap_used, clippy::expect_used)]

use crate::border_manager;
use crate::WindowManager;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub fn watch_for_orphans(wm: Arc<Mutex<WindowManager>>) {
    std::thread::spawn(move || loop {
        match find_orphans(wm.clone()) {
            Ok(()) => {
                tracing::warn!("restarting finished thread");
            }
            Err(error) => {
                if cfg!(debug_assertions) {
                    tracing::error!("restarting failed thread: {:?}", error)
                } else {
                    tracing::error!("restarting failed thread: {}", error)
                }
            }
        }
    });
}

pub fn find_orphans(wm: Arc<Mutex<WindowManager>>) -> color_eyre::Result<()> {
    tracing::info!("watching");

    let arc = wm.clone();

    loop {
        std::thread::sleep(Duration::from_secs(1));

        let mut wm = arc.lock();
        let offset = wm.work_area_offset;

        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            let work_area = *monitor.work_area_size();
            let window_based_work_area_offset = (
                monitor.window_based_work_area_offset_limit(),
                monitor.window_based_work_area_offset(),
            );

            let offset = if monitor.work_area_offset().is_some() {
                monitor.work_area_offset()
            } else {
                offset
            };

            for (j, workspace) in monitor.workspaces_mut().iter_mut().enumerate() {
                let reaped_orphans = workspace.reap_orphans()?;
                if reaped_orphans.0 > 0 || reaped_orphans.1 > 0 {
                    workspace.update(&work_area, offset, window_based_work_area_offset)?;
                    border_manager::event_tx().send(border_manager::Notification)?;
                    tracing::info!(
                        "reaped {} orphan window(s) and {} orphaned container(s) on monitor: {}, workspace: {}",
                        reaped_orphans.0,
                        reaped_orphans.1,
                        i,
                        j
                    );
                }
            }
        }
    }
}
