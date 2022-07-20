#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

use clap::Parser;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use color_eyre::eyre::anyhow;
use color_eyre::Report;
use color_eyre::Result;
use dirs::home_dir;
use json_dotpath::DotPaths;
use miow::pipe::NamedPipe;
use parking_lot::Mutex;
use serde_json::json;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;

use crate::configuration::Configuration;
use crate::configuration::Strategy;

mod configuration;

#[derive(Debug, Parser)]
struct Cli {
    #[clap(short = 'p', long)]
    kanata_port: i32,
    #[clap(short, long, default_value = "~/komokana.yaml")]
    configuration: String,
    #[clap(short, long)]
    default_layer: String,
    #[clap(short, long, action)]
    tmpfile: bool,
}

fn main() -> Result<()> {
    let cli: Cli = Cli::parse();
    let configuration = resolve_windows_path(&cli.configuration)?;

    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    color_eyre::install()?;
    env_logger::builder().format_timestamp(None).init();

    let mut komokana = Komokana::init(
        configuration,
        cli.kanata_port,
        cli.default_layer,
        cli.tmpfile,
    )?;

    komokana.listen();

    loop {
        sleep(Duration::from_secs(60));
    }
}

struct Komokana {
    komorebi: Arc<Mutex<NamedPipe>>,
    kanata: Arc<Mutex<TcpStream>>,
    configuration: Configuration,
    default_layer: String,
    tmpfile: bool,
}

const PIPE: &str = r#"\\.\pipe\"#;

impl Komokana {
    pub fn init(
        configuration: PathBuf,
        kanata_port: i32,
        default_layer: String,
        tmpfile: bool,
    ) -> Result<Self> {
        let name = "komokana";
        let pipe = format!("{}\\{}", PIPE, name);
        let mut cfg = home_dir().expect("could not look up home dir");
        cfg.push("komokana.yaml");

        let configuration: Configuration =
            serde_yaml::from_str(&std::fs::read_to_string(configuration)?)?;

        let named_pipe = NamedPipe::new(pipe)?;

        let mut output = Command::new("cmd.exe")
            .args(["/C", "komorebic.exe", "subscribe", name])
            .output()?;

        while !output.status.success() {
            log::warn!(
                "komorebic.exe failed with error code {:?}, retrying in 5 seconds...",
                output.status.code()
            );

            sleep(Duration::from_secs(5));

            output = Command::new("cmd.exe")
                .args(["/C", "komorebic.exe", "subscribe", name])
                .output()?;
        }

        named_pipe.connect()?;
        log::debug!("connected to komorebi");

        let stream = TcpStream::connect(format!("localhost:{kanata_port}"))?;
        log::debug!("connected to kanata");

        Ok(Self {
            komorebi: Arc::new(Mutex::new(named_pipe)),
            kanata: Arc::new(Mutex::new(stream)),
            configuration,
            default_layer,
            tmpfile,
        })
    }

    pub fn listen(&mut self) {
        let pipe = self.komorebi.clone();
        let stream = self.kanata.clone();
        let stream_read = self.kanata.clone();
        let tmpfile = self.tmpfile;
        log::info!("listening");

        thread::spawn(move || -> Result<()> {
            let mut read_stream = stream_read.lock().try_clone()?;
            drop(stream_read);

            loop {
                let mut buf = vec![0; 1024];
                if let Ok(bytes_read) = read_stream.read(&mut buf) {
                    let data = String::from_utf8(buf[0..bytes_read].to_vec())?;
                    if data == "\n" {
                        continue;
                    }

                    let notification: serde_json::Value = serde_json::from_str(&data)?;

                    if notification.dot_has("LayerChange.new") {
                        if let Some(new) = notification.dot_get::<String>("LayerChange.new")? {
                            log::info!("current layer: {new}");
                            if tmpfile {
                                let mut tmp = std::env::temp_dir();
                                tmp.push("kanata_layer");
                                std::fs::write(tmp, new)?;
                            }
                        }
                    }
                }
            }
        });

        let config = self.configuration.clone();
        let default_layer = self.default_layer.clone();
        thread::spawn(move || -> Result<()> {
            let mut buf = vec![0; 4096];
            loop {
                let mut named_pipe = pipe.lock();
                match (*named_pipe).read(&mut buf) {
                    Ok(bytes_read) => {
                        let data = String::from_utf8(buf[0..bytes_read].to_vec())?;
                        if data == "\n" {
                            continue;
                        }

                        let notification: serde_json::Value = serde_json::from_str(&data)?;
                        if notification.dot_has("event.content.1.exe") {
                            if let (Some(exe), Some(title), Some(kind)) = (
                                notification.dot_get::<String>("event.content.1.exe")?,
                                notification.dot_get::<String>("event.content.1.title")?,
                                notification.dot_get::<String>("event.type")?,
                            ) {
                                match kind.as_str() {
                                    "Show" => handle_event(
                                        &config,
                                        &stream,
                                        &default_layer,
                                        Event::Show,
                                        &exe,
                                        &title,
                                    )?,
                                    "FocusChange" => handle_event(
                                        &config,
                                        &stream,
                                        &default_layer,
                                        Event::FocusChange,
                                        &exe,
                                        &title,
                                    )?,
                                    _ => {}
                                };
                            }
                        }
                    }
                    Err(error) => {
                        // Broken pipe
                        if error.raw_os_error().expect("could not get raw os error") == 109 {
                            named_pipe.disconnect()?;

                            let mut output = Command::new("cmd.exe")
                                .args(["/C", "komorebic.exe", "subscribe", "bar"])
                                .output()?;

                            while !output.status.success() {
                                log::warn!(
                                    "komorebic.exe failed with error code {:?}, retrying in 5 seconds...",
                                    output.status.code()
                                );

                                sleep(Duration::from_secs(5));

                                output = Command::new("cmd.exe")
                                    .args(["/C", "komorebic.exe", "subscribe", "bar"])
                                    .output()?;
                            }

                            named_pipe.connect()?;
                        } else {
                            return Err(Report::from(error));
                        }
                    }
                }
            }
        });
    }
}

fn handle_event(
    configuration: &Configuration,
    stream: &Arc<Mutex<TcpStream>>,
    default_layer: &str,
    event: Event,
    exe: &str,
    title: &str,
) -> Result<()> {
    let target = calculate_target(
        configuration,
        event,
        exe,
        title,
        if matches!(event, Event::FocusChange) {
            Option::from(default_layer)
        } else {
            None
        },
    );

    if let Some(target) = target {
        let mut stream = stream.lock();
        let request = json!({
            "ChangeLayer": {
                "new": target,
            }
        });

        stream.write_all(request.to_string().as_bytes())?;
        log::debug!("request sent: {request}");
    };

    Ok(())
}

#[derive(Debug, Copy, Clone)]
pub enum Event {
    Show,
    FocusChange,
}

fn calculate_target(
    configuration: &Configuration,
    event: Event,
    exe: &str,
    title: &str,
    default: Option<&str>,
) -> Option<String> {
    let mut new_layer = default;
    for entry in configuration {
        if entry.exe == exe {
            if matches!(event, Event::FocusChange) {
                new_layer = Option::from(entry.target_layer.as_str());
            }

            if let Some(title_overrides) = &entry.title_overrides {
                for title_override in title_overrides {
                    match title_override.strategy {
                        Strategy::StartsWith => {
                            if title.starts_with(&title_override.title) {
                                new_layer = Option::from(title_override.target_layer.as_str());
                            }
                        }
                        Strategy::EndsWith => {
                            if title.ends_with(&title_override.title) {
                                new_layer = Option::from(title_override.target_layer.as_str());
                            }
                        }
                        Strategy::Contains => {
                            if title.contains(&title_override.title) {
                                new_layer = Option::from(title_override.target_layer.as_str());
                            }
                        }
                        Strategy::Equals => {
                            if title.eq(&title_override.title) {
                                new_layer = Option::from(title_override.target_layer.as_str());
                            }
                        }
                    }
                }

                // This acts like a default target layer within the application
                // which defaults back to the entry's main target layer
                if new_layer.is_none() {
                    new_layer = Option::from(entry.target_layer.as_str());
                }
            }

            if matches!(event, Event::FocusChange) {
                if let Some(virtual_key_overrides) = &entry.virtual_key_overrides {
                    for virtual_key_override in virtual_key_overrides {
                        if unsafe { GetKeyState(virtual_key_override.virtual_key_code) } < 0 {
                            new_layer = Option::from(virtual_key_override.targer_layer.as_str());
                        }
                    }
                }

                if let Some(virtual_key_ignores) = &entry.virtual_key_ignores {
                    for virtual_key in virtual_key_ignores {
                        if unsafe { GetKeyState(*virtual_key) } < 0 {
                            new_layer = None;
                        }
                    }
                }
            }
        }
    }

    new_layer.and_then(|new_layer| Option::from(new_layer.to_string()))
}

fn resolve_windows_path(raw_path: &str) -> Result<PathBuf> {
    let path = if raw_path.starts_with('~') {
        raw_path.replacen(
            '~',
            &dirs::home_dir()
                .ok_or_else(|| anyhow!("there is no home directory"))?
                .display()
                .to_string(),
            1,
        )
    } else {
        raw_path.to_string()
    };

    let full_path = PathBuf::from(path);

    let parent = full_path
        .parent()
        .ok_or_else(|| anyhow!("cannot parse directory"))?;

    let file = full_path
        .components()
        .last()
        .ok_or_else(|| anyhow!("cannot parse filename"))?;

    let mut canonicalized = std::fs::canonicalize(parent)?;
    canonicalized.push(file);

    Ok(canonicalized)
}
