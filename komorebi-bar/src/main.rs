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
mod widget;

use crate::bar::Komobar;
use crate::config::KomobarConfig;
use crate::config::Position;
use clap::Parser;
use eframe::egui::ViewportBuilder;
use komorebi_client::SocketMessage;
use schemars::gen::SchemaSettings;
use std::io::BufReader;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub static WIDGET_SPACING: f32 = 10.0;

#[derive(Parser)]
#[clap(author, about, version)]
struct Opts {
    /// Print the JSON schema of the configuration file and exit
    #[clap(long)]
    schema: bool,
    /// Path to a JSON or YAML configuration file
    #[clap(short, long)]
    config: Option<PathBuf>,
}

fn main() -> eframe::Result<()> {
    let opts: Opts = Opts::parse();

    if opts.schema {
        let settings = SchemaSettings::default().with(|s| {
            s.option_nullable = false;
            s.option_add_null_type = false;
            s.inline_subschemas = true;
        });

        let gen = settings.into_generator();
        let socket_message = gen.into_root_schema_for::<KomobarConfig>();
        let schema = serde_json::to_string_pretty(&socket_message).unwrap();

        println!("{schema}");
        std::process::exit(0);
    }

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

    let config_path = opts.config.map_or_else(
        || {
            let mut config = home_dir.join("komorebi.bar.json");
            if !config.is_file() {
                config.pop();
                config.push("komorebi.bar.yaml");
            }

            if !config.is_file() {
                None
            } else {
                Some(config)
            }
        },
        Option::from,
    );

    let config = match config_path {
        None => panic!(
            "no komorebi.bar.json or komorebi.bar.yaml found in {}",
            home_dir.as_path().to_string_lossy()
        ),
        Some(config) => KomobarConfig::read(&config).unwrap(),
    };

    let mut builder = ViewportBuilder::default()
        .with_decorations(false)
        // .with_transparent(config.transparent)
        .with_taskbar(false)
        .with_position(Position { x: 0.0, y: 0.0 })
        .with_inner_size({
            let state = serde_json::from_str::<komorebi_client::State>(
                &komorebi_client::send_query(&SocketMessage::State).unwrap(),
            )
            .unwrap();

            Position {
                x: state.monitors.elements()[config.monitor.index].size().right as f32,
                y: 20.0,
            }
        });

    if let Some(viewport) = &config.viewport {
        if let Some(position) = &viewport.position {
            let b = builder.clone();
            builder = b.with_position(*position);
        }

        if let Some(inner_size) = &viewport.inner_size {
            let b = builder.clone();
            builder = b.with_inner_size(*inner_size);
        }
    }

    let native_options = eframe::NativeOptions {
        viewport: builder,
        ..Default::default()
    };

    if let Some(rect) = &config.monitor.work_area_offset {
        komorebi_client::send_message(&SocketMessage::MonitorWorkAreaOffset(
            config.monitor.index,
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
