#[cfg(test)]
mod window_manager_tests {
    use color_eyre::eyre::anyhow;
    use crossbeam_channel::bounded;
    use crossbeam_channel::Receiver;
    use crossbeam_channel::Sender;
    use komorebi::monitor;
    use komorebi::Rect;
    use komorebi::WindowManagerEvent;
    use komorebi::{window_manager::WindowManager, DATA_DIR};
    use uuid::Uuid;

    #[test]
    fn test_create_window_manager() {
        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);
        let socket_name = format!("komorebi-test-{}.sock", Uuid::new_v4());
        let socket = Some(DATA_DIR.join(socket_name));
        let wm = WindowManager::new(receiver, socket.clone());
        assert!(wm.is_ok());

        if let Some(ref socket_path) = socket {
            let _ = std::fs::remove_file(socket_path);
        }
    }

    #[test]
    fn test_focus_workspace() {
        let (_sender, receiver): (Sender<WindowManagerEvent>, Receiver<WindowManagerEvent>) =
            bounded(1);
        let socket_name = format!("komorebi-test-{}.sock", Uuid::new_v4());
        let socket = Some(DATA_DIR.join(socket_name));
        let mut wm = WindowManager::new(receiver, socket.clone()).unwrap();
        let m = monitor::new(
            0,
            Rect::default(),
            Rect::default(),
            "TestMonitor".to_string(),
            "TestDevice".to_string(),
            "TestDeviceID".to_string(),
            Some("TestMonitorID".to_string()),
        );

        wm.monitors.elements_mut().push_back(m);

        let monitor_idx = {
            let monitor = wm
                .focused_monitor_mut()
                .ok_or_else(|| anyhow!("there is no workspace"))
                .unwrap();
            monitor.new_workspace_idx()
        };

        {
            let monitor = wm
                .focused_monitor_mut()
                .ok_or_else(|| anyhow!("there is no workspace"))
                .unwrap();
            monitor
                .focus_workspace(monitor_idx)
                .expect("failed to focus workspace");
        }

        assert_eq!(wm.focused_workspace_idx().unwrap(), 1);

        wm.focus_workspace(0).ok();

        assert_eq!(wm.focused_workspace_idx().unwrap(), 0);

        if let Some(ref socket_path) = socket {
            let _ = std::fs::remove_file(socket_path);
        }
    }
}
