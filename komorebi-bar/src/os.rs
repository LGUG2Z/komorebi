use egui_phosphor::regular::CALENDAR_DOTS;
use komorebi_bar_core::platform::BarPlatform;
use komorebi_bar_core::platform::SystemTool;
use std::io;
use std::process::Command;

pub struct WindowsBar;

impl BarPlatform for WindowsBar {
    fn open_system_tool(&self, tool: SystemTool) -> io::Result<()> {
        match tool {
            SystemTool::TaskManager => Command::new("cmd.exe").args(["/C", "taskmgr.exe"]).spawn(),
            SystemTool::PowerSettings => Command::new("cmd.exe")
                .args(["/C", "start", "ms-settings:batterysaver"])
                .spawn(),
            SystemTool::NetworkSettings => Command::new("cmd.exe").args(["/C", "ncpa"]).spawn(),
            SystemTool::Url(url) => Command::new("explorer.exe").args([url]).spawn(),
            SystemTool::StorageLocation(label) => Command::new("cmd.exe")
                .args(["/C", "explorer.exe", label.split(' ').collect::<Vec<&str>>()[0]])
                .spawn(),
        }
        .map(|_| ())
    }

    fn latest_release_api_url(&self) -> &'static str {
        "https://api.github.com/repos/LGUG2Z/komorebi/releases/latest"
    }

    fn calendar_icon(&self) -> &'static str {
        CALENDAR_DOTS
    }
}
