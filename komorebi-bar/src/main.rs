mod date;
mod memory;
mod storage;
mod time;
mod widget;

use crate::date::Date;
use crate::memory::Memory;
use crate::storage::Storage;
use crate::widget::BarWidget;
use crossbeam_channel::Receiver;
use eframe::egui;
use eframe::egui::Align;
use eframe::egui::CursorIcon;
use eframe::egui::Layout;
use eframe::egui::ViewportBuilder;
use eframe::egui::Visuals;
use komorebi_client::SocketMessage;
use std::io::BufRead;
use std::io::BufReader;
use std::process::Command;
use std::time::Duration;
use time::Time;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_decorations(false)
            // TODO: expose via config
            .with_transparent(true)
            // TODO: expose via config
            .with_position([0.0, 0.0])
            // TODO: expose via config
            .with_inner_size([5120.0, 20.0]),
        ..Default::default()
    };

    komorebi_client::send_message(&SocketMessage::MonitorWorkAreaOffset(
        0,
        komorebi_client::Rect {
            left: 0,
            top: 40,
            right: 0,
            bottom: 40,
        },
    ))
    .unwrap();

    let (tx_gui, rx_gui) = crossbeam_channel::unbounded();

    eframe::run_native(
        "komorebi-bar",
        native_options,
        Box::new(|cc| {
            let frame = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let listener = komorebi_client::subscribe("komorebi-bar").unwrap();

                for client in listener.incoming() {
                    match client {
                        Ok(subscription) => {
                            let reader = BufReader::new(subscription);

                            for line in reader.lines().flatten() {
                                if let Ok(notification) =
                                    serde_json::from_str::<komorebi_client::Notification>(&line)
                                {
                                    tx_gui.send(notification).unwrap();
                                    frame.request_repaint();
                                }
                            }
                        }
                        Err(error) => {
                            if error.raw_os_error().expect("could not get raw os error") == 109 {
                                while komorebi_client::send_message(
                                    &SocketMessage::AddSubscriberSocket(String::from(
                                        "komorebi-bar",
                                    )),
                                )
                                .is_err()
                                {
                                    std::thread::sleep(Duration::from_secs(5));
                                }
                            }
                        }
                    }
                }
            });

            Ok(Box::new(Komobar::new(cc, rx_gui)))
        }),
    )
}

struct Komobar {
    state_receiver: Receiver<komorebi_client::Notification>,
    selected_workspace: String,
    workspaces: Vec<String>,
    time: Time,
    date: Date,
    memory: Memory,
    storage: Storage,
}

impl Komobar {
    fn new(_cc: &eframe::CreationContext<'_>, rx: Receiver<komorebi_client::Notification>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.

        Self {
            state_receiver: rx,
            selected_workspace: String::new(),
            workspaces: vec![],
            time: Time::default(),
            date: Date::default(),
            memory: Memory::default(),
            storage: Storage::default(),
        }
    }
}

impl Komobar {
    fn handle_komorebi_notification(&mut self) {
        if let Ok(notification) = self.state_receiver.try_recv() {
            self.workspaces = {
                let mut workspaces = vec![];
                // TODO: komobar only operates on the 0th monitor (for now)
                let monitor = &notification.state.monitors.elements()[0];
                let focused_workspace_idx = monitor.focused_workspace_idx();
                self.selected_workspace = monitor.workspaces()[focused_workspace_idx]
                    .name()
                    .to_owned()
                    .unwrap_or_else(|| format!("{}", focused_workspace_idx + 1));

                for (i, ws) in monitor.workspaces().iter().enumerate() {
                    workspaces.push(ws.name().to_owned().unwrap_or_else(|| format!("{}", i + 1)));
                }

                workspaces
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
                    .outer_margin(egui::Margin::symmetric(10.0, 10.0)),
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
                    });

                    // TODO: make the order configurable
                    // TODO: make each widget optional??
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for time in self.time.output() {
                            ctx.request_repaint();
                            if ui
                                .label(format!("🕐 {}", time))
                                .on_hover_cursor(CursorIcon::default())
                                .clicked()
                            {
                                // TODO: make default format configurable
                                self.time.format.toggle()
                            }
                        }

                        // TODO: make spacing configurable
                        ui.add_space(10.0);

                        for date in self.date.output() {
                            if ui
                                .label(format!("📅 {}", date))
                                .on_hover_cursor(CursorIcon::default())
                                .clicked()
                            {
                                // TODO: make default format configurable
                                self.date.format.next()
                            };
                        }

                        // TODO: make spacing configurable
                        ui.add_space(10.0);

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
                    })
                })
            });
    }
}
