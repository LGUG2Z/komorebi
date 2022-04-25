mod bar;
mod date;
mod ip_address;
mod ram;
mod storage;
mod time;
mod widget;
mod workspaces;

use crate::ip_address::IpAddress;
use crate::ram::Ram;
use crate::storage::Storage;
use bar::Bar;
use color_eyre::Result;
use date::Date;
use date::DateFormat;
use eframe::run_native;
use eframe::NativeOptions;
use egui::Color32;
use egui::Pos2;
use egui::Vec2;
use komorebi::WindowsApi;
use time::Time;
use time::TimeFormat;
use windows::Win32::Graphics::Gdi::HMONITOR;
use workspaces::Workspaces;

fn main() -> Result<()> {
    let workspaces = Workspaces::init(0)?;
    let time = Time::init(TimeFormat::TwentyFourHour);
    let date = Date::init(DateFormat::DayDateMonthYear);
    let ip_address = IpAddress::init(String::from("Ethernet"));

    let app = Bar {
        background_rgb: Color32::from_rgb(255, 0, 0),
        text_rgb: Color32::from_rgb(255, 255, 255),
        workspaces,
        time,
        date,
        ip_address,
        memory: Ram,
        storage: Storage,
    };

    let mut win_option = NativeOptions {
        decorated: false,
        ..Default::default()
    };

    // let hmonitors = WindowsApi::valid_hmonitors()?;
    // for hmonitor in hmonitors {
    //     let info = WindowsApi::monitor_info(hmonitor)?;
    // }

    let info = WindowsApi::monitor_info_w(HMONITOR(65537))?;

    let offset = Offsets {
        vertical: 10.0,
        horizontal: 200.0,
    };

    win_option.initial_window_pos = Option::from(Pos2::new(
        info.rcWork.left as f32 + offset.horizontal,
        info.rcWork.top as f32 + offset.vertical * 2.0,
    ));

    win_option.initial_window_size = Option::from(Vec2::new(
        info.rcWork.right as f32 - (offset.horizontal * 2.0),
        info.rcWork.top as f32 - offset.vertical,
    ));

    win_option.always_on_top = true;

    run_native(Box::new(app), win_option);
}

struct Offsets {
    vertical: f32,
    horizontal: f32,
}
