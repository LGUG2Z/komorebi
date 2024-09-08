mod battery;
mod date;
mod komorebi;
mod media;
mod memory;
mod network;
mod storage;
mod time;
mod widget;

use crate::battery::Battery;
use crate::battery::BatteryConfig;
use crate::date::Date;
use crate::date::DateConfig;
use crate::date::DateFormat;
use crate::komorebi::Komorebi;
use crate::komorebi::KomorebiConfig;
use crate::komorebi::KomorebiFocusedWindowConfig;
use crate::komorebi::KomorebiLayoutConfig;
use crate::komorebi::KomorebiWorkspacesConfig;
use crate::media::Media;
use crate::media::MediaConfig;
use crate::memory::Memory;
use crate::memory::MemoryConfig;
use crate::network::Network;
use crate::network::NetworkConfig;
use crate::storage::Storage;
use crate::storage::StorageConfig;
use crate::time::TimeConfig;
use crate::time::TimeFormat;
use crate::widget::BarWidget;
use crossbeam_channel::Receiver;
use eframe::egui;
use eframe::egui::Align;
use eframe::egui::ColorImage;
use eframe::egui::Context;
use eframe::egui::Layout;
use eframe::egui::TextureHandle;
use eframe::egui::ViewportBuilder;
use eframe::emath::Pos2;
use eframe::emath::Vec2;
use font_loader::system_fonts;
use font_loader::system_fonts::FontPropertyBuilder;
use image::RgbaImage;
use komorebi_client::SocketMessage;
use std::cell::RefCell;
use std::io::BufReader;
use std::io::Read;
use std::ops::Deref;
use std::rc::Rc;
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

#[derive(Clone, Debug)]
pub struct Config {
    inner_size: Position,
    position: Position,
    outer_margin: Position,
    monitor_index: usize,
    monitor_work_area_offset: Option<komorebi_client::Rect>,
    font_family: Option<String>,
    time: TimeConfig,
    date: DateConfig,
    storage: StorageConfig,
    memory: MemoryConfig,
    media: MediaConfig,
    battery: BatteryConfig,
    network: NetworkConfig,
    komorebi: KomorebiConfig,
    theme: Theme,
}

#[derive(Copy, Clone, Debug)]
pub enum Theme {
    Default,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
}

fn main() -> eframe::Result<()> {
    let config = Config {
        inner_size: Position { x: 5120.0, y: 20.0 },
        position: Position { x: 0.0, y: 0.0 },
        outer_margin: Position { x: 10.0, y: 10.0 },
        monitor_index: 0,
        font_family: Some(String::from("JetBrains Mono")),
        monitor_work_area_offset: Some(komorebi_client::Rect {
            left: 0,
            top: 40,
            right: 0,
            bottom: 40,
        }),
        time: TimeConfig {
            enable: true,
            format: TimeFormat::TwentyFourHour,
        },
        date: DateConfig {
            enable: true,
            format: DateFormat::DayDateMonthYear,
        },
        storage: StorageConfig { enable: true },
        memory: MemoryConfig { enable: true },
        media: MediaConfig { enable: true },
        battery: BatteryConfig { enable: true },
        network: NetworkConfig {
            enable: true,
            show_data: true,
        },
        komorebi: KomorebiConfig {
            enable: true,
            monitor_index: 0,
            workspaces: KomorebiWorkspacesConfig {
                enable: true,
                hide_empty_workspaces: true,
            },
            layout: KomorebiLayoutConfig { enable: true },
            focused_window: KomorebiFocusedWindowConfig {
                enable: true,
                show_icon: true,
            },
        },
        theme: Theme::CatppuccinMacchiato,
    };

    // TODO: ensure that config.monitor_index represents a valid komorebi monitor index

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_decorations(false)
            // .with_transparent(config.transparent)
            .with_position(config.position)
            .with_taskbar(false)
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
            let config_cl = config_arc.clone();

            let ctx_repainter = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                ctx_repainter.request_repaint();
            });

            let ctx_komorebi = cc.egui_ctx.clone();
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
                                ctx_komorebi.request_repaint();
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
    komorebi_notification_state: Rc<RefCell<KomorebiNotificationState>>,
    left_widgets: Vec<Box<dyn BarWidget>>,
    right_widgets: Vec<Box<dyn BarWidget>>,
    rx_gui: Receiver<komorebi_client::Notification>,
}

#[derive(Clone, Debug)]
struct KomorebiNotificationState {
    monitor_index: usize,
    workspaces: Vec<String>,
    selected_workspace: String,
    focused_window_title: String,
    focused_window_pid: Option<u32>,
    focused_window_icon: Option<RgbaImage>,
    layout: String,
    hide_empty_workspaces: bool,
}

impl KomorebiNotificationState {
    fn handle_notification(&mut self, rx_gui: Receiver<komorebi_client::Notification>) {
        if let Ok(notification) = rx_gui.try_recv() {
            let monitor = &notification.state.monitors.elements()[self.monitor_index];
            let focused_workspace_idx = monitor.focused_workspace_idx();

            let mut workspaces = vec![];
            self.selected_workspace = monitor.workspaces()[focused_workspace_idx]
                .name()
                .to_owned()
                .unwrap_or_else(|| format!("{}", focused_workspace_idx + 1));

            for (i, ws) in monitor.workspaces().iter().enumerate() {
                let should_add = if self.hide_empty_workspaces {
                    focused_workspace_idx == i || !ws.containers().is_empty()
                } else {
                    true
                };

                if should_add {
                    workspaces.push(ws.name().to_owned().unwrap_or_else(|| format!("{}", i + 1)));
                }
            }

            self.workspaces = workspaces;
            self.layout = match monitor.workspaces()[focused_workspace_idx].layout() {
                komorebi_client::Layout::Default(layout) => layout.to_string(),
                komorebi_client::Layout::Custom(_) => String::from("Custom"),
            };

            if let Some(container) = monitor.workspaces()[focused_workspace_idx].focused_container()
            {
                if let Some(window) = container.focused_window() {
                    if let Ok(title) = window.title() {
                        self.focused_window_title.clone_from(&title);
                        self.focused_window_pid = Some(window.process_id());
                        let img = windows_icons::get_icon_by_process_id(window.process_id());
                        self.focused_window_icon = Some(img);
                    }
                }
            } else {
                self.focused_window_title.clear();
                self.focused_window_icon = None;
            }

            if let Some(container) = monitor.workspaces()[focused_workspace_idx].monocle_container()
            {
                if let Some(window) = container.focused_window() {
                    if let Ok(title) = window.title() {
                        self.focused_window_title.clone_from(&title);
                    }
                }
            }

            if let Some(window) = monitor.workspaces()[focused_workspace_idx].maximized_window() {
                if let Ok(title) = window.title() {
                    self.focused_window_title.clone_from(&title);
                }
            }
        }
    }
}

fn add_custom_font(ctx: &Context, name: &str) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    let property = FontPropertyBuilder::new().family(name).build();

    if let Some((font, _)) = system_fonts::get(&property) {
        // Install my own font (maybe supporting non-latin characters).
        // .ttf and .otf files supported.
        fonts
            .font_data
            .insert(name.to_owned(), egui::FontData::from_owned(font));

        // Put my font first (highest priority) for proportional text:
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, name.to_owned());

        // Put my font as last fallback for monospace:
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push(name.to_owned());

        // Tell egui to use these fonts:
        ctx.set_fonts(fonts);
    }
}
impl Komobar {
    fn new(
        cc: &eframe::CreationContext<'_>,
        rx: Receiver<komorebi_client::Notification>,
        config: Arc<Config>,
    ) -> Self {
        if let Some(font_family) = &config.font_family {
            add_custom_font(&cc.egui_ctx, font_family);
        }

        match config.theme {
            Theme::Default => {}
            Theme::CatppuccinFrappe => {
                catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::FRAPPE);
            }
            Theme::CatppuccinMacchiato => {
                catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MACCHIATO);
            }
            Theme::CatppuccinMocha => {
                catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MOCHA);
            }
        }

        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        let komorebi_workspaces = Komorebi::from(config.komorebi);
        let komorebi_notification_state = komorebi_workspaces.komorebi_notification_state.clone();

        let left_widgets: Vec<Box<dyn BarWidget>> = vec![Box::new(komorebi_workspaces.clone())];

        let mut right_widgets: Vec<Box<dyn BarWidget>> = vec![
            Box::new(Media::from(config.media)),
            Box::new(Storage::from(config.storage)),
            Box::new(Memory::from(config.memory)),
            Box::new(Network::from(config.network)),
            Box::new(Date::from(config.date)),
            Box::new(Time::from(config.time)),
            Box::new(Battery::from(config.battery)),
        ];

        right_widgets.reverse();

        Self {
            config: config.deref().clone(),
            komorebi_notification_state,
            left_widgets,
            right_widgets,
            rx_gui: rx,
        }
    }
}
fn img_to_texture(ctx: &Context, rgba_image: &RgbaImage) -> TextureHandle {
    let size = [rgba_image.width() as usize, rgba_image.height() as usize];
    let pixels = rgba_image.as_flat_samples();
    let color_image = ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
    ctx.load_texture("icon", color_image, egui::TextureOptions::default())
}

impl eframe::App for Komobar {
    // TODO: I think this is needed for transparency??
    // fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
    // egui::Rgba::TRANSPARENT.to_array()
    // let mut background = Color32::from_gray(18).to_normalized_gamma_f32();
    // background[3] = 0.9;
    // background
    // }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.komorebi_notification_state
            .borrow_mut()
            .handle_notification(self.rx_gui.clone());

        egui::CentralPanel::default()
            .frame(egui::Frame::none().outer_margin(egui::Margin::symmetric(
                self.config.outer_margin.x,
                self.config.outer_margin.y,
            )))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        for w in &mut self.left_widgets {
                            w.render(ctx, ui);
                        }
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for w in &mut self.right_widgets {
                            w.render(ctx, ui);
                        }
                    })
                })
            });
    }
}
