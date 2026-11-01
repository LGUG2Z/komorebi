use crate::macos::widgets::ImageIcon;
use egui_phosphor::regular::CALENDAR;
use komorebi_bar_core::platform::BarPlatform;
use komorebi_bar_core::platform::SystemTool;
use komorebi_client::SocketMessage;
use std::io;
use std::process::Command;

pub struct MacBar;

impl BarPlatform for MacBar {
    fn open_system_tool(&self, tool: SystemTool) -> io::Result<()> {
        let (program, args) = system_tool_command(&tool);
        Command::new(program).args(args).spawn().map(|_| ())
    }

    fn latest_release_api_url(&self) -> &'static str {
        "https://api.github.com/repos/LGUG2Z/komorebi-for-mac/releases/latest"
    }

    fn release_page_url(&self, version: &str) -> String {
        format!("https://github.com/LGUG2Z/komorebi-for-mac/releases/tag/v{version}")
    }

    fn calendar_icon(&self) -> &'static str {
        CALENDAR
    }

    fn exec_shell_command(&self, _command: &str) -> color_eyre::Result<()> {
        // the macOS bar has no shell session: the mouse commands are not supported here
        Err(io::Error::new(io::ErrorKind::Unsupported, "no shell session on macOS").into())
    }

    fn send_komorebi_message(&self, message: &SocketMessage) -> io::Result<()> {
        komorebi_client::send_message(message)
    }

    fn send_komorebi_batch(&self, messages: Vec<SocketMessage>) -> io::Result<()> {
        komorebi_client::send_batch(messages)
    }

    fn command_icon(&self, _command: &str) -> Option<ImageIcon> {
        None
    }
}

/// The program and arguments that open `tool` on macOS: everything goes through `open`.
fn system_tool_command(tool: &SystemTool) -> (&'static str, Vec<String>) {
    let args: Vec<String> = match tool {
        SystemTool::TaskManager => vec!["-a".into(), "Activity Monitor".into()],
        // System Settings of macOS 13 and later (the same scheme as the network pane)
        SystemTool::PowerSettings => {
            vec!["x-apple.systempreferences:com.apple.Battery-Settings.extension".into()]
        }
        SystemTool::NetworkSettings => {
            vec!["x-apple.systempreferences:com.apple.Network-Settings.extension".into()]
        }
        SystemTool::Url(url) => vec![url.clone()],
        SystemTool::StorageLocation(label) => {
            // the label is "<path> <usage>": the path is everything but the last word
            let parts: Vec<&str> = label.split(' ').collect();
            vec![parts[..parts.len() - 1].join(" ")]
        }
    };

    ("open", args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(tool: SystemTool) -> Vec<String> {
        let (program, args) = system_tool_command(&tool);
        assert_eq!(
            program, "open",
            "nothing on macOS is started by another program"
        );
        args
    }

    #[test]
    fn the_task_manager_is_activity_monitor() {
        assert_eq!(argv(SystemTool::TaskManager), ["-a", "Activity Monitor"]);
    }

    #[test]
    fn the_battery_opens_the_battery_pane_of_system_settings() {
        assert_eq!(
            argv(SystemTool::PowerSettings),
            ["x-apple.systempreferences:com.apple.Battery-Settings.extension"]
        );
    }

    #[test]
    fn the_network_opens_the_network_pane_of_system_settings() {
        assert_eq!(
            argv(SystemTool::NetworkSettings),
            ["x-apple.systempreferences:com.apple.Network-Settings.extension"]
        );
    }

    #[test]
    fn a_url_is_opened_in_the_default_browser() {
        assert_eq!(
            argv(SystemTool::Url("https://example.com/a b".into())),
            ["https://example.com/a b"]
        );
    }

    #[test]
    fn a_storage_label_opens_its_path_without_the_free_space() {
        assert_eq!(
            argv(SystemTool::StorageLocation("/Volumes/My Disk 43%".into())),
            ["/Volumes/My Disk"]
        );
    }

    #[test]
    fn the_release_page_is_the_one_of_the_mac_repository() {
        assert_eq!(
            MacBar.release_page_url("0.1.2"),
            "https://github.com/LGUG2Z/komorebi-for-mac/releases/tag/v0.1.2"
        );
    }
}
