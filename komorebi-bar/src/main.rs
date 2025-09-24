mod bar;
mod config;
mod render;
mod selected_frame;
mod ui;
mod widgets;

use crate::bar::Komobar;
use crate::config::KomobarConfig;
use crate::config::Position;
use crate::config::PositionConfig;
use clap::Parser;
use config::MonitorConfigOrIndex;
use eframe::egui::ViewportBuilder;
use font_loader::system_fonts;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use komorebi_client::PathExt;
use komorebi_client::SocketMessage;
use komorebi_client::SubscribeOptions;
use komorebi_client::replace_env_in_path;
use std::io::BufReader;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
use windows::Win32::UI::WindowsAndMessaging::EnumThreadWindows;
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows_core::BOOL;

pub static MAX_LABEL_WIDTH: AtomicI32 = AtomicI32::new(400);
pub static MONITOR_LEFT: AtomicI32 = AtomicI32::new(0);
pub static MONITOR_TOP: AtomicI32 = AtomicI32::new(0);
pub static MONITOR_RIGHT: AtomicI32 = AtomicI32::new(0);
pub static MONITOR_INDEX: AtomicUsize = AtomicUsize::new(0);
pub static BAR_HEIGHT: f32 = 50.0;
pub static DEFAULT_PADDING: f32 = 10.0;

pub static AUTO_SELECT_FILL_COLOUR: AtomicU32 = AtomicU32::new(0);
pub static AUTO_SELECT_TEXT_COLOUR: AtomicU32 = AtomicU32::new(0);

#[derive(Parser)]
#[clap(author, about, version)]
struct Opts {
    /// Print the JSON schema of the configuration file and exit
    #[clap(long)]
    schema: bool,
    /// Print a list of fonts available on this system and exit
    #[clap(long)]
    fonts: bool,
    /// Path to a JSON or YAML configuration file
    #[clap(short, long)]
    #[clap(value_parser = replace_env_in_path)]
    config: Option<PathBuf>,
    /// Write an example komorebi.bar.json to disk
    #[clap(long)]
    quickstart: bool,
    /// Print a list of aliases that can be renamed to canonical variants
    #[clap(long)]
    #[clap(hide = true)]
    aliases: bool,
}

extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));

        if process_id == GetCurrentProcessId() {
            *(lparam.0 as *mut HWND) = hwnd;
            BOOL::from(false) // Stop enumeration
        } else {
            BOOL::from(true) // Continue enumeration
        }
    }
}

fn process_hwnd() -> Option<isize> {
    unsafe {
        let mut hwnd = HWND::default();
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(enum_window),
            LPARAM(&mut hwnd as *mut HWND as isize),
        );

        if hwnd.0 as isize == 0 {
            None
        } else {
            Some(hwnd.0 as isize)
        }
    }
}

pub enum KomorebiEvent {
    Notification(komorebi_client::Notification),
    Reconnect,
}

fn main() -> color_eyre::Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }?;

    let opts: Opts = Opts::parse();

    #[cfg(feature = "schemars")]
    if opts.schema {
        let settings = schemars::r#gen::SchemaSettings::default().with(|s| {
            s.option_nullable = false;
            s.option_add_null_type = false;
            s.inline_subschemas = true;
        });

        let generator = settings.into_generator();
        let socket_message = generator.into_root_schema_for::<KomobarConfig>();
        let schema = serde_json::to_string_pretty(&socket_message)?;

        println!("{schema}");
        std::process::exit(0);
    }

    if opts.fonts {
        for font in system_fonts::query_all() {
            println!("{font}");
        }

        std::process::exit(0);
    }

    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        unsafe {
            std::env::set_var("RUST_LIB_BACKTRACE", "1");
        }
    }

    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        unsafe {
            std::env::set_var("RUST_LOG", "info");
        }
    }

    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt::Subscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .finish(),
    )?;

    let home_dir: PathBuf = std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
        |_| dirs::home_dir().expect("there is no home directory"),
        |home_path| {
            let home = home_path.replace_env();

            assert!(
                home.is_dir(),
                "$Env:KOMOREBI_CONFIG_HOME is set to '{home_path}', which is not a valid directory"
            );

            home
        },
    );

    if opts.quickstart {
        let komorebi_bar_json = include_str!("../../docs/komorebi.bar.example.json").to_string();
        std::fs::write(home_dir.join("komorebi.bar.json"), komorebi_bar_json)?;
        println!(
            "Example komorebi.bar.json file written to {}",
            home_dir.display()
        );

        std::process::exit(0);
    }

    let default_config_path = home_dir.join("komorebi.bar.json");

    let config_path = opts.config.or_else(|| {
        default_config_path
            .is_file()
            .then_some(default_config_path.clone())
    });

    let mut config = match config_path {
        None => {
            let komorebi_bar_json =
                include_str!("../../docs/komorebi.bar.example.json").to_string();

            std::fs::write(&default_config_path, komorebi_bar_json)?;
            tracing::info!(
                "created example configuration file: {}",
                default_config_path.display()
            );

            KomobarConfig::read(&default_config_path)?
        }
        Some(ref config) => {
            if !opts.aliases {
                tracing::info!("found configuration file: {}", config.display());
            }

            KomobarConfig::read(config)?
        }
    };

    let config_path = config_path.unwrap_or(default_config_path);

    if opts.aliases {
        KomobarConfig::aliases(&std::fs::read_to_string(&config_path)?);
        std::process::exit(0);
    }

    let state = serde_json::from_str::<komorebi_client::State>(&komorebi_client::send_query(
        &SocketMessage::State,
    )?)?;

    let (usr_monitor_index, work_area_offset) = match &config.monitor {
        MonitorConfigOrIndex::MonitorConfig(monitor_config) => {
            (monitor_config.index, monitor_config.work_area_offset)
        }
        MonitorConfigOrIndex::Index(idx) => (*idx, None),
    };
    let monitor_index = state
        .monitor_usr_idx_map
        .get(&usr_monitor_index)
        .map_or(usr_monitor_index, |i| *i);

    MONITOR_RIGHT.store(
        state.monitors.elements()[monitor_index].size.right,
        Ordering::SeqCst,
    );

    MONITOR_TOP.store(
        state.monitors.elements()[monitor_index].size.top,
        Ordering::SeqCst,
    );

    MONITOR_LEFT.store(
        state.monitors.elements()[monitor_index].size.left,
        Ordering::SeqCst,
    );

    MONITOR_INDEX.store(monitor_index, Ordering::SeqCst);

    match config.position {
        None => {
            config.position = Some(PositionConfig {
                start: Some(Position {
                    x: state.monitors.elements()[monitor_index].size.left as f32,
                    y: state.monitors.elements()[monitor_index].size.top as f32,
                }),
                end: Some(Position {
                    x: state.monitors.elements()[monitor_index].size.right as f32,
                    y: 50.0,
                }),
            })
        }
        Some(ref mut position) => {
            if position.start.is_none() {
                position.start = Some(Position {
                    x: state.monitors.elements()[monitor_index].size.left as f32,
                    y: state.monitors.elements()[monitor_index].size.top as f32,
                });
            }

            if position.end.is_none() {
                position.end = Some(Position {
                    x: state.monitors.elements()[monitor_index].size.right as f32,
                    y: 50.0,
                })
            }
        }
    }

    let viewport_builder = ViewportBuilder::default()
        .with_decorations(false)
        .with_transparent(true)
        .with_taskbar(false);

    let native_options = eframe::NativeOptions {
        viewport: viewport_builder,
        ..Default::default()
    };

    if let Some(rect) = &work_area_offset {
        komorebi_client::send_message(&SocketMessage::MonitorWorkAreaOffset(monitor_index, *rect))?;
        tracing::info!("work area offset applied to monitor: {}", monitor_index);
    }

    let (tx_gui, rx_gui) = crossbeam_channel::unbounded();
    let (tx_config, rx_config) = crossbeam_channel::unbounded();

    let mut hotwatch = Hotwatch::new()?;
    let config_path_cl = config_path.clone();

    hotwatch.watch(config_path, move |event| match event.kind {
        EventKind::Modify(_) | EventKind::Remove(_) => match KomobarConfig::read(&config_path_cl) {
            Ok(updated) => {
                tracing::info!("configuration file updated: {}", config_path_cl.display());

                if let Err(error) = tx_config.send(updated) {
                    tracing::error!("could not send configuration update to gui: {error}")
                }
            }
            Err(error) => {
                tracing::error!("{error}");
            }
        },
        _ => {}
    })?;

    tracing::info!("watching configuration file for changes");

    eframe::run_native(
        "komorebi-bar",
        native_options,
        Box::new(|cc| {
            let ctx_repainter = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                ctx_repainter.request_repaint();
            });

            let ctx_komorebi = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let subscriber_name = format!("komorebi-bar-{}", random_word::get(random_word::Lang::En));

                let listener = komorebi_client::subscribe_with_options(&subscriber_name, SubscribeOptions {
                    filter_state_changes: true,
                })
                    .expect("could not subscribe to komorebi notifications");

                tracing::info!("subscribed to komorebi notifications: \"{}\"", subscriber_name);

                for client in listener.incoming() {
                    match client {
                        Ok(subscription) => {
                            match subscription.set_read_timeout(Some(Duration::from_secs(1))) {
                                Ok(()) => {}
                                Err(error) => tracing::error!("{}", error),
                            }
                            let mut buffer = Vec::new();
                            let mut reader = BufReader::new(subscription);

                            // this is when we know a shutdown has been sent
                            if matches!(reader.read_to_end(&mut buffer), Ok(0)) {
                                tracing::info!("disconnected from komorebi");

                                // keep trying to reconnect to komorebi
                                while komorebi_client::send_message(
                                    &SocketMessage::AddSubscriberSocket(subscriber_name.clone()),
                                )
                                    .is_err()
                                {
                                    std::thread::sleep(Duration::from_secs(1));
                                }

                                tracing::info!("reconnected to komorebi");

                                if let Err(error) = tx_gui.send(KomorebiEvent::Reconnect) {
                                    tracing::error!("could not send komorebi reconnect event to gui thread: {error}")
                                }

                                ctx_komorebi.request_repaint();
                                continue;
                            }

                            match String::from_utf8(buffer) {
                                Ok(notification_string) => {
                                    match serde_json::from_str::<komorebi_client::Notification>(
                                        &notification_string,
                                    ) {
                                        Ok(notification) => {
                                            tracing::debug!("received notification from komorebi");

                                            if let Err(error) = tx_gui.send(KomorebiEvent::Notification(notification)) {
                                                tracing::error!("could not send komorebi notification update to gui thread: {error}")
                                            }

                                            ctx_komorebi.request_repaint();
                                        }
                                        Err(error) => {
                                            tracing::error!("could not deserialize komorebi notification: {error}");
                                        }
                                    }
                                }
                                Err(error) => {
                                    tracing::error!(
                                        "komorebi notification string was invalid utf8: {error}"
                                    )
                                }
                            }
                        }
                        Err(error) => {
                            tracing::error!("{error}");
                        }
                    }
                }
            });

            Ok(Box::new(Komobar::new(cc, rx_gui, rx_config, config)))
        }),
    )
        .map_err(|error| color_eyre::eyre::Error::msg(error.to_string()))
}
