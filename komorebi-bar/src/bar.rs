use crate::date::Date;
use crate::ram::Ram;
use crate::time::Time;
use crate::widget::BarWidget;
use crate::widget::Output;
use crate::widget::Widget;
use crate::IpAddress;
use crate::Storage;
use crate::Workspaces;
use clipboard_win::set_clipboard_string;
use color_eyre::owo_colors::OwoColorize;
use eframe::epi::App;
use eframe::epi::Frame;
use egui::style::Margin;
use egui::CentralPanel;
use egui::Color32;
use egui::Context;
use egui::Direction;
use egui::Layout;
use egui::Rounding;
use std::process::Command;
use std::sync::atomic::Ordering;

pub struct Bar {
    pub background_rgb: Color32,
    pub text_rgb: Color32,
    pub workspaces: Workspaces,
    pub time: Time,
    pub date: Date,
    pub ip_address: IpAddress,
    pub memory: Ram,
    pub storage: Storage,
}

impl App for Bar {
    fn update(&mut self, ctx: &Context, frame: &Frame) {
        let custom_frame = egui::Frame {
            margin: Margin::symmetric(8.0, 8.0),
            rounding: Rounding::none(),
            fill: self.background_rgb,
            ..Default::default()
        };

        CentralPanel::default().frame(custom_frame).show(ctx, |ui| {
            ui.horizontal(|horizontal| {
                horizontal.style_mut().visuals.override_text_color = Option::from(self.text_rgb);

                horizontal.with_layout(Layout::left_to_right(), |ltr| {
                    for (i, workspace) in self.workspaces.output().iter().enumerate() {
                        if workspace == "komorebi offline" {
                            ltr.label(workspace);
                        } else {
                            ctx.request_repaint();
                            if ltr
                                .selectable_label(*self.workspaces.selected.lock() == i, workspace)
                                .clicked()
                            {
                                let mut selected = self.workspaces.selected.lock();
                                *selected = i;

                                if let Err(error) = Workspaces::focus(i) {
                                    eprintln!("{}", error)
                                };
                            }
                        }
                    }
                });

                horizontal.with_layout(Layout::right_to_left(), |rtl| {
                    for time in self.time.output() {
                        ctx.request_repaint();
                        if rtl.button(format!("🕐 {}", time)).clicked() {
                            self.time.format.toggle()
                        };
                    }

                    for date in self.date.output() {
                        if rtl.button(format!("📅 {}", date)).clicked() {
                            self.date.format.next()
                        };
                    }

                    for memory in self.memory.output() {
                        if rtl.button(format!("🐏 {}", memory)).clicked() {
                            if let Err(error) =
                                Command::new("cmd.exe").args(["/C", "taskmgr.exe"]).output()
                            {
                                eprintln!("{}", error)
                            }
                        };
                    }

                    for disk in self.storage.output() {
                        if rtl.button(format!("🖴 {}", disk)).clicked() {
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
                    }

                    for ip in self.ip_address.output() {
                        if rtl.button(format!("🌐 {}", ip)).clicked() {
                            if let Err(error) =
                                Command::new("cmd.exe").args(["/C", "ncpa.cpl"]).output()
                            {
                                eprintln!("{}", error)
                            }
                        };
                    }
                });
            })
        });
    }

    fn name(&self) -> &str {
        "komorebi-bar"
    }
}
