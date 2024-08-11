mod date;
mod memory;
mod storage;
mod time;
mod widget;

use crate::date::Date;
use crate::date::DateFormat;
use crate::memory::Memory;
use crate::memory::MemoryConfig;
use crate::storage::Storage;
use crate::storage::StorageConfig;
use crate::time::TimeFormat;
use crate::widget::BarWidget;
use crossbeam_channel::Receiver;
use eframe::egui;
use eframe::egui::Align;
use eframe::egui::CursorIcon;
use eframe::egui::Layout;
use eframe::egui::ViewportBuilder;
use eframe::egui::Visuals;
use eframe::emath::Pos2;
use eframe::emath::Vec2;
use komorebi_client::SocketMessage;
use std::io::BufReader;
use std::io::Read;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use time::Time;

#[derive(Copy, Clone, Debug)]
pub struct Position {
    x: f32,
    y: f32,
}

impl From<Position> for Vec2 {
    fn from(value: Position) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<Position> for Pos2 {
    fn from(value: Position) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Config {
    inner_size: Position,
    position: Position,
    outer_margin: Position,
    transparent: bool,
    monitor_index: usize,
    monitor_work_area_offset: Option<komorebi_client::Rect>,
    time: Time,
    date: Date,
    storage: StorageConfig,
    memory: MemoryConfig,
}

fn main() -> eframe::Result<()> {
    let config = Config {
        inner_size: Position { x: 5120.0, y: 20.0 },
        position: Position { x: 0.0, y: 0.0 },
        outer_margin: Position { x: 10.0, y: 10.0 },
        transparent: false,
        monitor_index: 0,
        monitor_work_area_offset: Some(komorebi_client::Rect {
            left: 0,
            top: 40,
            right: 0,
            bottom: 40,
        }),
        time: Time::new(true, TimeFormat::TwentyFourHour),
        date: Date::new(true, DateFormat::DayDateMonthYear),
        storage: StorageConfig { enable: true },
        memory: MemoryConfig { enable: true },
    };

    // TODO: ensure that config.monitor_index represents a valid komorebi monitor index

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(config.transparent)
            .with_position(config.position)
            .with_inner_size(config.inner_size),
        ..Default::default()
    };

    if let Some(rect) = &config.monitor_work_area_offset {
        komorebi_client::send_message(&SocketMessage::MonitorWorkAreaOffset(
            config.monitor_index,
            *rect,
        ))
        .unwrap();
    }

    let (tx_gui, rx_gui) = crossbeam_channel::unbounded();
    let config_arc = Arc::new(config);

    eframe::run_native(
        "komorebi-bar",
        native_options,
        Box::new(|cc| {
            let frame = cc.egui_ctx.clone();
            let config_cl = config_arc.clone();

            std::thread::spawn(move || {
                let listener = komorebi_client::subscribe("komorebi-bar").unwrap();

                for client in listener.incoming() {
                    match client {
                        Ok(subscription) => {
                            let mut buffer = Vec::new();
                            let mut reader = BufReader::new(subscription);

                            // this is when we know a shutdown has been sent
                            if matches!(reader.read_to_end(&mut buffer), Ok(0)) {
                                // keep trying to reconnect to komorebi
                                while komorebi_client::send_message(
                                    &SocketMessage::AddSubscriberSocket(String::from(
                                        "komorebi-bar",
                                    )),
                                )
                                .is_err()
                                {
                                    std::thread::sleep(Duration::from_secs(1));
                                }

                                // here we have reconnected
                                if let Some(rect) = &config_cl.monitor_work_area_offset {
                                    while komorebi_client::send_message(
                                        &SocketMessage::MonitorWorkAreaOffset(
                                            config_cl.monitor_index,
                                            *rect,
                                        ),
                                    )
                                    .is_err()
                                    {
                                        std::thread::sleep(Duration::from_secs(1));
                                    }
                                }
                            }

                            if let Ok(notification) =
                                serde_json::from_str::<komorebi_client::Notification>(
                                    &String::from_utf8(buffer).unwrap(),
                                )
                            {
                                tx_gui.send(notification).unwrap();
                                frame.request_repaint();
                            }
                        }
                        Err(error) => {
                            dbg!(error);
                        }
                    }
                }
            });

            Ok(Box::new(Komobar::new(cc, rx_gui, config_arc)))
        }),
    )
}

struct Komobar {
    config: Config,
    state_receiver: Receiver<komorebi_client::Notification>,
    selected_workspace: String,
    focused_window_title: String,
    workspace_layout: String,
    workspaces: Vec<String>,
    time: Time,
    date: Date,
    memory: Memory,
    storage: Storage,
}

impl Komobar {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        rx: Receiver<komorebi_client::Notification>,
        config: Arc<Config>,
    ) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.

        Self {
            config: *config,
            state_receiver: rx,
            selected_workspace: String::new(),
            focused_window_title: String::new(),
            workspace_layout: String::new(),
            workspaces: vec![],
            time: config.time,
            date: config.date,
            memory: Memory::from(config.memory),
            storage: Storage::from(config.storage),
        }
    }
}

impl Komobar {
    fn handle_komorebi_notification(&mut self) {
        if let Ok(notification) = self.state_receiver.try_recv() {
            let monitor = &notification.state.monitors.elements()[self.config.monitor_index];
            let focused_workspace_idx = monitor.focused_workspace_idx();

            let mut workspaces = vec![];
            self.selected_workspace = monitor.workspaces()[focused_workspace_idx]
                .name()
                .to_owned()
                .unwrap_or_else(|| format!("{}", focused_workspace_idx + 1));

            for (i, ws) in monitor.workspaces().iter().enumerate() {
                workspaces.push(ws.name().to_owned().unwrap_or_else(|| format!("{}", i + 1)));
            }

            self.workspaces = workspaces;
            self.workspace_layout = match monitor.workspaces()[focused_workspace_idx].layout() {
                komorebi_client::Layout::Default(layout) => layout.to_string(),
                komorebi_client::Layout::Custom(_) => String::from("Custom"),
            };

            if let Some(container) = monitor.workspaces()[focused_workspace_idx].focused_container()
            {
                if let Some(window) = container.focused_window() {
                    if let Ok(title) = window.title() {
                        self.focused_window_title.clone_from(&title);
                    }
                }
            }
        }
    }
}

impl eframe::App for Komobar {
    // TODO: I think this is needed for transparency??
    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
        let mut background = egui::Color32::from_gray(18).to_normalized_gamma_f32();
        background[3] = 0.9;
        background
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_komorebi_notification();

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    // TODO: make this configurable
                    .outer_margin(egui::Margin::symmetric(
                        self.config.outer_margin.x,
                        self.config.outer_margin.y,
                    )),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        // TODO: maybe this should be a widget??
                        for (i, ws) in self.workspaces.iter().enumerate() {
                            if ui
                                .add(egui::SelectableLabel::new(
                                    self.selected_workspace.eq(ws),
                                    ws.to_string(),
                                ))
                                .clicked()
                            {
                                self.selected_workspace = ws.to_string();
                                komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                                    false,
                                ))
                                .unwrap();
                                komorebi_client::send_message(
                                    &SocketMessage::FocusWorkspaceNumber(i),
                                )
                                .unwrap();
                                // TODO: store MFF value from state and restore that here instead of "true"
                                komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(
                                    true,
                                ))
                                .unwrap();
                                komorebi_client::send_message(&SocketMessage::Retile).unwrap();
                            }
                        }

                        ui.label(&self.workspace_layout);

                        ui.add_space(10.0);

                        ui.label(&self.focused_window_title);

                        ui.add_space(10.0);
                    });

                    // TODO: make the order configurable
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.time.enable {
                            for time in self.time.output() {
                                ctx.request_repaint();
                                if ui
                                    .label(format!("🕐 {}", time))
                                    .on_hover_cursor(CursorIcon::default())
                                    .clicked()
                                {
                                    self.time.format.toggle()
                                }
                            }

                            // TODO: make spacing configurable
                            ui.add_space(10.0);
                        }

                        if self.date.enable {
                            for date in self.date.output() {
                                if ui
                                    .label(format!("📅 {}", date))
                                    .on_hover_cursor(CursorIcon::default())
                                    .clicked()
                                {
                                    self.date.format.next()
                                };
                            }

                            // TODO: make spacing configurable
                            ui.add_space(10.0);
                        }

                        if self.memory.enable {
                            for ram in self.memory.output() {
                                if ui
                                    // TODO: make label configurable??
                                    .label(format!("🐏 {}", ram))
                                    .on_hover_cursor(CursorIcon::default())
                                    .clicked()
                                {
                                    if let Err(error) =
                                        Command::new("cmd.exe").args(["/C", "taskmgr.exe"]).output()
                                    {
                                        eprintln!("{}", error)
                                    }
                                };
                            }

                            ui.add_space(10.0);
                        }

                        if self.storage.enable {
                            for disk in self.storage.output() {
                                if ui
                                    // TODO: Make emoji configurable??
                                    .label(format!("🖴 {}", disk))
                                    .on_hover_cursor(CursorIcon::default())
                                    .clicked()
                                {
                                    if let Err(error) = Command::new("cmd.exe")
                                        .args([
                                            "/C",
                                            "explorer.exe",
                                            disk.split(' ').collect::<Vec<&str>>()[0],
                                        ])
                                        .output()
                                    {
                                        eprintln!("{}", error)
                                    }
                                };

                                ui.add_space(10.0);
                            }
                        }
                    })
                })
            });
    }
}
