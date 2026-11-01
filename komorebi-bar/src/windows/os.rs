use crate::windows::widgets::ImageIcon;
use egui_phosphor::regular::CALENDAR_DOTS;
use komorebi_bar_core::platform::BarPlatform;
use komorebi_bar_core::platform::SystemTool;
use komorebi_client::SocketMessage;
use std::borrow::Cow;
use std::io;
use std::path::Path;
use std::process::Command;
use which::which;

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
                .args([
                    "/C",
                    "explorer.exe",
                    label.split(' ').collect::<Vec<&str>>()[0],
                ])
                .spawn(),
        }
        .map(|_| ())
    }

    fn latest_release_api_url(&self) -> &'static str {
        "https://api.github.com/repos/LGUG2Z/komorebi/releases/latest"
    }

    fn release_page_url(&self, version: &str) -> String {
        format!("https://github.com/LGUG2Z/komorebi/releases/v{version}")
    }

    fn calendar_icon(&self) -> &'static str {
        CALENDAR_DOTS
    }

    fn exec_shell_command(&self, command: &str) -> color_eyre::Result<()> {
        crate::windows::bar::exec_powershell(command)
    }

    fn send_komorebi_message(&self, message: &SocketMessage) -> io::Result<()> {
        komorebi_client::send_message(message)
    }

    fn send_komorebi_batch(&self, messages: Vec<SocketMessage>) -> io::Result<()> {
        komorebi_client::send_batch(messages)
    }

    fn command_icon(&self, command: &str) -> Option<ImageIcon> {
        let path = executable_of(command)?;
        ImageIcon::try_load(path.as_ref(), || {
            let path_str = path.to_str()?;
            windows_icons::get_icon_by_path(path_str)
                .or_else(|| windows_icons_fallback::get_icon_by_path(path_str))
        })
    }
}

/// Attempts to resolve the executable path from the command string.
///
/// Resolution logic:
/// - Splits the command by ".exe" and checks if the first part is an existing file.
/// - If not, attempts to locate the binary using [`which`] on this name.
/// - If still unresolved, takes the first word (separated by whitespace) and attempts
///   to find it in the system `PATH` using [`which`].
///
/// Returns `None` if no executable path can be determined.
fn executable_of(command: &str) -> Option<Cow<'_, Path>> {
    if let Some(binary) = command.split(".exe").next().map(Path::new) {
        if binary.is_file() {
            return Some(Cow::Borrowed(binary));
        } else if let Ok(binary) = which(binary) {
            return Some(Cow::Owned(binary));
        }
    }

    which(command.split(' ').next()?).ok().map(Cow::Owned)
}
