mod bar;
mod battery;
mod config;
mod date;
mod komorebi;
mod media;
mod memory;
mod network;
mod storage;
mod time;
mod ui;
mod widget;

use crate::bar::Komobar;
use crate::config::KomobarConfig;
use crate::config::Position;
use atomic_float::AtomicF32;
use clap::Parser;
use color_eyre::eyre::bail;
use eframe::egui::ViewportBuilder;
use font_loader::system_fonts;
use hotwatch::EventKind;
use hotwatch::Hotwatch;
use komorebi_client::SocketMessage;
use schemars::gen::SchemaSettings;
use std::io::BufReader;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;

pub static WIDGET_SPACING: f32 = 10.0;
pub static MAX_LABEL_WIDTH: AtomicI32 = AtomicI32::new(400);
pub static DPI: AtomicF32 = AtomicF32::new(1.0);

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
    config: Option<PathBuf>,
    /// Write an example komorebi.bar.json to disk
    #[clap(long)]
    quickstart: bool,
}

macro_rules! as_ptr {
    ($value:expr) => {
        $value as *mut core::ffi::c_void
    };
}

pub fn dpi_for_monitor(hmonitor: isize) -> color_eyre::Result<f32> {
    use windows::Win32::Graphics::Gdi::HMONITOR;
    use windows::Win32::UI::HiDpi::GetDpiForMonitor;
    use windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI;

    let mut dpi_x = u32::default();
    let mut dpi_y = u32::default();

    unsafe {
        match GetDpiForMonitor(
            HMONITOR(as_ptr!(hmonitor)),
            MDT_EFFECTIVE_DPI,
            std::ptr::addr_of_mut!(dpi_x),
            std::ptr::addr_of_mut!(dpi_y),
        ) {
            Ok(_) => {}
            Err(error) => bail!(error),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    Ok(dpi_y as f32 / 96.0)
}

fn main() -> color_eyre::Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }?;

    let opts: Opts = Opts::parse();

    if opts.schema {
        let settings = SchemaSettings::default().with(|s| {
            s.option_nullable = false;
            s.option_add_null_type = false;
            s.inline_subschemas = true;
        });

        let gen = settings.into_generator();
        let socket_message = gen.into_root_schema_for::<KomobarConfig>();
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
        std::env::set_var("RUST_LIB_BACKTRACE", "1");
    }

    color_eyre::install()?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt::Subscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .finish(),
    )?;

    let home_dir: PathBuf = std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
        |_| dirs::home_dir().expect("there is no home directory"),
        |home_path| {
            let home = PathBuf::from(&home_path);

            if home.as_path().is_dir() {
                home
            } else {
                panic!("$Env:KOMOREBI_CONFIG_HOME is set to '{home_path}', which is not a valid directory");
            }
        },
    );

    if opts.quickstart {
        let komorebi_bar_json = include_str!("../../docs/komorebi.bar.example.json").to_string();
        std::fs::write(home_dir.join("komorebi.bar.json"), komorebi_bar_json)?;
        println!(
            "Example komorebi.bar.json file written to {}",
            home_dir.as_path().display()
        );

        std::process::exit(0);
    }

    let default_config_path = home_dir.join("komorebi.bar.json");

    let config_path = opts.config.map_or_else(
        || {
            if !default_config_path.is_file() {
                None
            } else {
                Some(default_config_path.clone())
            }
        },
        Option::from,
    );

    let config = match config_path {
        None => {
            let komorebi_bar_json =
                include_str!("../../docs/komorebi.bar.example.json").to_string();

            std::fs::write(&default_config_path, komorebi_bar_json)?;
            tracing::info!(
                "created example configuration file: {}",
                default_config_path.as_path().display()
            );

            KomobarConfig::read(&default_config_path)?
        }
        Some(ref config) => {
            tracing::info!(
                "found configuration file: {}",
                config.as_path().to_string_lossy()
            );

            KomobarConfig::read(config)?
        }
    };

    let config_path = config_path.unwrap_or(default_config_path);

    let state = serde_json::from_str::<komorebi_client::State>(&komorebi_client::send_query(
        &SocketMessage::State,
    )?)?;

    let dpi = dpi_for_monitor(state.monitors.elements()[config.monitor.index].id())?;
    DPI.store(dpi, Ordering::SeqCst);

    let mut viewport_builder = ViewportBuilder::default()
        .with_decorations(false)
        // .with_transparent(config.transparent)
        .with_taskbar(false)
        .with_position(Position {
            x: state.monitors.elements()[config.monitor.index].size().left as f32 / dpi,
            y: state.monitors.elements()[config.monitor.index].size().top as f32 / dpi,
        })
        .with_inner_size({
            Position {
                x: state.monitors.elements()[config.monitor.index].size().right as f32 / dpi,
                y: 50.0 / dpi,
            }
        });

    if let Some(viewport) = &config.viewport {
        if let Some(mut position) = &viewport.position {
            position.x /= dpi;
            position.y /= dpi;

            let b = viewport_builder.clone();
            viewport_builder = b.with_position(position);
        }

        if let Some(mut inner_size) = &viewport.inner_size {
            inner_size.x /= dpi;
            inner_size.y /= dpi;

            let b = viewport_builder.clone();
            viewport_builder = b.with_inner_size(inner_size);
        }
    }

    let native_options = eframe::NativeOptions {
        viewport: viewport_builder,
        ..Default::default()
    };

    if let Some(rect) = &config.monitor.work_area_offset {
        komorebi_client::send_message(&SocketMessage::MonitorWorkAreaOffset(
            config.monitor.index,
            *rect,
        ))?;
        tracing::info!(
            "work area offset applied to monitor: {}",
            config.monitor.index
        );
    }

    let (tx_gui, rx_gui) = crossbeam_channel::unbounded();
    let (tx_config, rx_config) = crossbeam_channel::unbounded();

    let mut hotwatch = Hotwatch::new()?;
    let config_path_cl = config_path.clone();

    hotwatch.watch(config_path, move |event| match event.kind {
        EventKind::Modify(_) | EventKind::Remove(_) => match KomobarConfig::read(&config_path_cl) {
            Ok(updated) => {
                tracing::info!(
                    "configuration file updated: {}",
                    config_path_cl.as_path().to_string_lossy()
                );

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
                let subscriber_name = format!("komorebi-bar-{}", random_word::gen(random_word::Lang::En));

                let listener = komorebi_client::subscribe(&subscriber_name)
                    .expect("could not subscribe to komorebi notifications");

                tracing::info!("subscribed to komorebi notifications: \"{}\"", subscriber_name);

                for client in listener.incoming() {
                    match client {
                        Ok(subscription) => {
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

                                if let Some(rect) = &config_cl.monitor.work_area_offset {
                                    while komorebi_client::send_message(
                                        &SocketMessage::MonitorWorkAreaOffset(
                                            config_cl.monitor.index,
                                            *rect,
                                        ),
                                    )
                                    .is_err()
                                    {
                                        std::thread::sleep(Duration::from_secs(1));
                                    }
                                }
                            }

                            match String::from_utf8(buffer) {
                                Ok(notification_string) => {
                                    if let Ok(notification) =
                                        serde_json::from_str::<komorebi_client::Notification>(
                                            &notification_string,
                                        )
                                    {
                                        tracing::debug!("received notification from komorebi");

                                        if let Err(error) = tx_gui.send(notification) {
                                            tracing::error!("could not send komorebi notification update to gui: {error}")
                                        }

                                        ctx_komorebi.request_repaint();
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

            Ok(Box::new(Komobar::new(cc, rx_gui, rx_config, config_arc)))
        }),
    )
    .map_err(|error| color_eyre::eyre::Error::msg(error.to_string()))
}
