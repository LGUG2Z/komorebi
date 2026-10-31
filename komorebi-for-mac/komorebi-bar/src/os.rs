use egui_phosphor::regular::CALENDAR;
use komorebi_bar_core::platform::BarPlatform;
use komorebi_bar_core::platform::SystemTool;
use std::io;
use std::process::Command;

pub struct MacBar;

impl BarPlatform for MacBar {
    fn open_system_tool(&self, tool: SystemTool) -> io::Result<()> {
        match tool {
            SystemTool::TaskManager => Command::new("open")
                .args(["-a", "Activity Monitor"])
                .spawn(),
            // cmd.exe does not exist on macOS: this is what the battery widget does today
            SystemTool::PowerSettings => Command::new("cmd.exe")
                .args(["/C", "start", "ms-settings:batterysaver"])
                .spawn(),
            SystemTool::NetworkSettings => Command::new("open")
                .args(["x-apple.systempreferences:com.apple.Network-Settings.extension"])
                .spawn(),
            // explorer.exe does not exist on macOS: this is what the update widget does today
            SystemTool::Url(url) => Command::new("explorer.exe").args([url]).spawn(),
        }
        .map(|_| ())
    }

    fn latest_release_api_url(&self) -> &'static str {
        "https://api.github.com/repos/LGUG2Z/komorebi-for-mac/releases/latest"
    }

    fn calendar_icon(&self) -> &'static str {
        CALENDAR
    }
}
