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
        let mut update_borders = false;

        for (i, monitor) in wm.monitors_mut().iter_mut().enumerate() {
            for (j, workspace) in monitor.workspaces_mut().iter_mut().enumerate() {
                let reaped_orphans = workspace.reap_orphans()?;
                if reaped_orphans.0 > 0 || reaped_orphans.1 > 0 {
                    workspace.update()?;
                    update_borders = true;
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

        if update_borders {
            border_manager::send_notification(None);
        }
    }
}
