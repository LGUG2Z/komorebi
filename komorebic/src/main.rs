#![warn(clippy::all)]
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]
#![allow(unused_assignments)] // false positives for the error reporter

use chrono::Utc;
use komorebi_client::PathExt;
use komorebi_client::splash;

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::bail;
use fs_tail::TailedFile;
use komorebi_client::AppSpecificConfigurationPath;
use komorebi_client::ApplicationSpecificConfiguration;
use komorebi_client::send_message;
use komorebi_client::send_query;
use komorebic_core::cli::Cmd;
use komorebic_core::cli::Meta;
use komorebic_core::cli::command;
use komorebic_core::docgen::DocFormat;
use komorebic_core::launcher;
use komorebic_core::launcher::Launcher;
use komorebic_core::messages::Messages;
use komorebic_core::pan;
use komorebic_core::platform::windows::*;
use lazy_static::lazy_static;
use miette::NamedSource;
use miette::Report;
use miette::SourceOffset;
use miette::SourceSpan;
use serde::Deserialize;
use sysinfo::ProcessesToUpdate;
use which::which;
use win_msgbox::OkayCancel;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
use windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;
use windows::Win32::UI::WindowsAndMessaging::ShowWindow;

use komorebi_client::ApplicationConfigurationGenerator;
use komorebi_client::ApplicationIdentifier;
use komorebi_client::Rect;
use komorebi_client::SocketMessage;
use komorebi_client::StaticConfig;
use komorebi_client::splash::ValidationFeedback;

lazy_static! {
    static ref HAS_CUSTOM_CONFIG_HOME: AtomicBool = AtomicBool::new(false);
    static ref HOME_DIR: PathBuf = {
        std::env::var("KOMOREBI_CONFIG_HOME").map_or_else(
            |_| dirs::home_dir().expect("there is no home directory"),
            |home_path| {
                let home = home_path.replace_env();
                if home.as_path().is_dir() {
                    HAS_CUSTOM_CONFIG_HOME.store(true, Ordering::SeqCst);
                    home
                } else {
                    panic!(
                        "$Env:KOMOREBI_CONFIG_HOME is set to '{home_path}', which is not a valid directory",
                    );
                }
            },
        )
    };
    static ref DATA_DIR: PathBuf = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi");
    static ref WHKD_CONFIG_DIR: PathBuf = {
        std::env::var("WHKD_CONFIG_HOME").map_or_else(
            |_| {
                dirs::home_dir()
                    .expect("there is no home directory")
                    .join(".config")
            },
            |home_path| {
                let whkd_config_home = home_path.replace_env();

                assert!(
                    whkd_config_home.is_dir(),
                    "$Env:WHKD_CONFIG_HOME is set to '{home_path}', which is not a valid directory"
                );

                whkd_config_home
            },
        )
    };
}

shadow_rs::shadow!(build);

#[derive(thiserror::Error, Debug, miette::Diagnostic)]
#[error("{message}")]
#[diagnostic(code(komorebi::configuration), help("try fixing this syntax error"))]
struct ConfigurationError {
    message: String,
    #[source_code]
    src: NamedSource<String>,
    #[label("This bit here")]
    bad_bit: SourceSpan,
}

// print_query is a helper that queries komorebi and prints the response.
// panics on error.
fn print_query(message: &SocketMessage) {
    match send_query(message) {
        Ok(response) => println!("{response}"),
        Err(error) => panic!("{}", error),
    }
}

fn startup_dir() -> eyre::Result<PathBuf> {
    let startup = dirs::home_dir()
        .expect("unable to obtain user's home folder")
        .join("AppData")
        .join("Roaming")
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup");

    if !startup.is_dir() {
        std::fs::create_dir_all(&startup)?;
    }

    Ok(startup)
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
/// Starting `komorebi.exe` for `komorebic start`: a PowerShell script that starts the daemon with
/// its flags, found among the processes by its name.
struct WindowsLauncher {
    script: String,
    config: Option<PathBuf>,
    system: sysinfo::System,
}

impl Launcher for WindowsLauncher {
    fn daemon_name(&self) -> &str {
        "komorebi.exe"
    }

    fn spawn(&mut self) -> eyre::Result<()> {
        match powershell_script::run(&self.script) {
            Ok(_) => {
                println!("{}", self.script);
            }
            Err(error) => {
                println!("Error: {error}");
            }
        }

        Ok(())
    }

    fn is_running(&mut self) -> eyre::Result<bool> {
        self.system.refresh_processes(ProcessesToUpdate::All, true);

        Ok(self
            .system
            .processes_by_name("komorebi.exe".as_ref())
            .next()
            .is_some())
    }

    fn diagnose(&mut self) -> eyre::Result<()> {
        if let Some(config) = &self.config {
            if let Ok(output) = Command::new("komorebi.exe")
                .arg(format!("'--config=\"{}\"'", config.display()))
                .output()
            {
                println!("{}", String::from_utf8(output.stderr)?);
            }
        } else if let Ok(output) = Command::new("komorebi.exe").output() {
            println!("{}", String::from_utf8(output.stderr)?);
        }

        Ok(())
    }
}

/// What the top of the help and `--version` say.
const META: Meta = Meta {
    name: "komorebic",
    about: env!("CARGO_PKG_DESCRIPTION"),
    author: env!("CARGO_PKG_AUTHORS"),
    version: build::CLAP_LONG_VERSION,
};

/// Write a message to the daemon's socket, or, if the daemon answers it, print the answer.
fn send_or_query(messages: Messages) -> eyre::Result<()> {
    let query = messages.is_query();
    let message = SocketMessage::from(messages);
    if query {
        print_query(&message);
    } else {
        send_message(&message)?;
    }
    Ok(())
}

/// The connection to the daemon that the gesture stream tells what the fingers do over.
struct PanLink(komorebi_client::Connection);

impl pan::Link for PanLink {
    fn send(&mut self, message: &SocketMessage) -> io::Result<()> {
        self.0.send(message)
    }
}

fn main() -> eyre::Result<()> {
    let opts = komorebic_core::cli::parse::<WindowsCommands>(&META);

    match opts.cmd {
        Cmd::Messages(messages) => send_or_query(messages)?,
        Cmd::Platform(platform) => match platform {
            WindowsCommands::Docgen(args) => {
                let output_dir = args.output.unwrap_or_else(|| PathBuf::from("docs/cli"));
                let pages = komorebic_core::docgen::write(
                    command::<WindowsCommands>(&META),
                    &DocFormat::PLAIN,
                    &output_dir,
                )?;
                for page in pages {
                    println!("{}", page.display());
                }
            }
            WindowsCommands::Splash(args) => {
                let informative_text = match args.mdm_server {
                None => {
                    "It looks like you are using a corporate device enrolled in mobile device management\n\n\
                         The Komorebi License does not permit any kind of commercial use\n\n\
                         A dedicated Individual Commercial Use License is available if you wish to use this software at work\n\n\
                         You are strongly encouraged to make your employer pay for your license, either directly or via reimbursement\n\n\
                         To remove this popup in the future, run \"komorebic license <email>\" using the email address associated with your license\n\n\
                         If you are a student using komorebi on a laptop provided by your school, you can email me from your educational email account to remove this popup".to_string()
                }
                Some(server) => {
                    format!(
                        "It looks like you are using a corporate device enrolled in mobile device management ({server})\n\n\
                             The Komorebi License does not permit any kind of commercial use\n\n\
                             A dedicated Individual Commercial Use License is available if you wish to use this software at work\n\n\
                             You are strongly encouraged to make your employer pay for your license, either directly or via reimbursement\n\n\
                             To remove this popup in the future you can run \"komorebic license <email>\" using the email address associated with your license\n\n
                             If you are a student using komorebi on a laptop provided by your school, you can email me from your educational email account to remove this popup"
                    )
                }
            };

                if let Ok(response) = win_msgbox::error::<OkayCancel>(&informative_text)
                    .title("MDM Enrollment Detected")
                    .topmost()
                    .icon(win_msgbox::Icon::Warning)
                    .set_foreground()
                    .show()
                {
                    match response {
                        OkayCancel::Okay => {
                            open::that("https://lgug2z.com/software/komorebi")?;
                        }
                        OkayCancel::Cancel => {}
                    }
                }
            }
            WindowsCommands::License(args) => {
                let _ = std::fs::remove_file(DATA_DIR.join("icul.validation"));
                std::fs::write(DATA_DIR.join("icul"), &args.email)?;
                match splash::should()? {
                    ValidationFeedback::Successful(icul_validation) => {
                        println!("Individual commercial use license validation successful");
                        println!(
                            "Local validation file saved to {}",
                            icul_validation.display()
                        );
                        println!("\n{}", std::fs::read_to_string(&icul_validation)?);
                    }
                    ValidationFeedback::Unsuccessful(invalid_payload) => {
                        println!(
                            "No active individual commercial use license found for {}",
                            args.email
                        );
                        println!("\n{invalid_payload}");
                        println!(
                            "\nYou can purchase an individual commercial use license at https://lgug2z.com/software/komorebi"
                        );
                    }
                    ValidationFeedback::NoEmail => {}
                    ValidationFeedback::NoConnectivity => {
                        println!(
                            "Could not make a connection to validate an individual commercial use license for {}",
                            args.email
                        );
                    }
                }
            }
            WindowsCommands::Quickstart => {
                fn write_file_with_prompt(
                    path: &PathBuf,
                    content: &str,
                    created_files: &mut Vec<String>,
                ) -> eyre::Result<()> {
                    if path.exists() {
                        print!(
                            "{} will be overwritten, do you want to continue? (y/N): ",
                            path.display()
                        );
                        io::stdout().flush()?;
                        let mut input = String::new();
                        io::stdin().read_line(&mut input)?;
                        let trimmed = input.trim().to_lowercase();
                        if trimmed == "y" || trimmed == "yes" {
                            std::fs::write(path, content)?;
                            created_files.push(path.display().to_string());
                        } else {
                            println!("Skipping {}", path.display());
                        }
                    } else {
                        std::fs::write(path, content)?;
                        created_files.push(path.display().to_string());
                    }
                    Ok(())
                }

                let local_appdata_dir =
                    dirs::data_local_dir().expect("could not find localdata dir");
                let data_dir = local_appdata_dir.join("komorebi");
                std::fs::create_dir_all(&*WHKD_CONFIG_DIR)?;
                std::fs::create_dir_all(&*HOME_DIR)?;
                std::fs::create_dir_all(data_dir)?;

                let mut komorebi_json =
                    include_str!("../../docs/komorebi.example.json").to_string();
                let komorebi_bar_json =
                    include_str!("../../docs/komorebi.bar.example.json").to_string();

                if std::env::var("KOMOREBI_CONFIG_HOME").is_ok() {
                    komorebi_json =
                        komorebi_json.replace("Env:USERPROFILE", "Env:KOMOREBI_CONFIG_HOME");
                }

                let komorebi_path = HOME_DIR.join("komorebi.json");
                let bar_path = HOME_DIR.join("komorebi.bar.json");
                let applications_path = HOME_DIR.join("applications.json");
                let whkdrc_path = WHKD_CONFIG_DIR.join("whkdrc");

                let mut written_files = Vec::new();

                write_file_with_prompt(&komorebi_path, &komorebi_json, &mut written_files)?;
                write_file_with_prompt(&bar_path, &komorebi_bar_json, &mut written_files)?;

                let applications_json = include_str!("../applications.json");
                write_file_with_prompt(&applications_path, applications_json, &mut written_files)?;

                let whkdrc = include_str!("../../docs/whkdrc.sample");
                write_file_with_prompt(&whkdrc_path, whkdrc, &mut written_files)?;
                if written_files.is_empty() {
                    println!("\nNo files were written.")
                } else {
                    println!(
                        "\nThe following example files were written:\n{}",
                        written_files.join("\n")
                    );
                }

                if let Ok((mdm, server)) = splash::mdm_enrollment()
                    && mdm
                {
                    if let Some(server) = server {
                        println!(
                            "\nIt looks like you are using a corporate device enrolled in mobile device management ({server})"
                        );
                    } else {
                        println!(
                            "\nIt looks like you are using a corporate device enrolled in mobile device management"
                        );
                    }
                    println!("The Komorebi License does not permit any kind of commercial use");
                    println!(
                        "A dedicated Individual Commercial Use License is available if you wish to use this software at work"
                    );
                    println!(
                        "You are strongly encouraged to make your employer pay for your license, either directly or via reimbursement"
                    );
                    println!(
                        "If you already have a license, you can run \"komorebic license <email>\" with the email address your license is associated with"
                    );
                }

                println!("\nYou can now run komorebic start --whkd --bar");
            }
            WindowsCommands::EnableAutostart(args) => {
                if args.ahk {
                    println!(
                        "EOL: The --ahk flag is now end-of-life and will not receive any further updates or bug fixes"
                    );
                }

                let mut current_exe = std::env::current_exe().expect("unable to get exec path");
                current_exe.pop();
                let komorebic_exe = current_exe.join("komorebic-no-console.exe");
                let komorebic_exe = dunce::simplified(&komorebic_exe);

                let startup_dir = startup_dir()?;
                let shortcut_file = startup_dir.join("komorebi.lnk");
                let shortcut_file = dunce::simplified(&shortcut_file);

                let mut arguments = String::from("start");

                if let Some(config) = args.config {
                    arguments.push_str(" --config ");
                    arguments.push_str(&config.to_string_lossy());
                }

                if args.ffm {
                    arguments.push_str(" --ffm");
                }

                if args.bar {
                    arguments.push_str(" --bar");
                }

                if args.whkd {
                    arguments.push_str(" --whkd");
                } else if args.ahk {
                    arguments.push_str(" --ahk");
                }

                if args.masir {
                    arguments.push_str(" --masir");
                }

                Command::new("powershell")
                .arg("-c")
                .arg("$WshShell = New-Object -comObject WScript.Shell; $Shortcut = $WshShell.CreateShortcut($env:SHORTCUT_PATH); $Shortcut.TargetPath = $env:TARGET_PATH; $Shortcut.Arguments = $env:TARGET_ARGS; $Shortcut.Save()")
                .env("SHORTCUT_PATH", shortcut_file.as_os_str())
                .env("TARGET_PATH", komorebic_exe.as_os_str())
                .env("TARGET_ARGS", arguments)
                .output()?;

                println!(
                    "NOTE: If your komorebi.json file contains a reference to $Env:KOMOREBI_CONFIG_HOME,"
                );
                println!(
                    "you need to add this to System Properties > Environment Variables > User Variables"
                );
                println!("in order for the autostart command to work properly");
            }
            WindowsCommands::DisableAutostart => {
                let startup_dir = startup_dir()?;
                let shortcut_file = startup_dir.join("komorebi.lnk");

                if shortcut_file.is_file() {
                    std::fs::remove_file(shortcut_file)?;
                }
            }
            WindowsCommands::Check(args) => {
                let home_display = HOME_DIR.display();
                if HAS_CUSTOM_CONFIG_HOME.load(Ordering::SeqCst) {
                    println!("KOMOREBI_CONFIG_HOME detected: {home_display}\n");
                } else {
                    println!(
                        "No KOMOREBI_CONFIG_HOME detected, defaulting to {}\n",
                        dirs::home_dir()
                            .expect("could not find home dir")
                            .to_string_lossy()
                    );
                }

                println!("Looking for configuration files in {home_display}\n");

                let static_config = if let Some(static_config) = args.komorebi_config {
                    println!(
                        "Using an arbitrary configuration file passed to --komorebi-config flag\n"
                    );
                    static_config
                } else {
                    HOME_DIR.join("komorebi.json")
                };

                let config_pwsh = HOME_DIR.join("komorebi.ps1");
                let config_ahk = HOME_DIR.join("komorebi.ahk");
                let config_whkd = WHKD_CONFIG_DIR.join("whkdrc");

                if static_config.exists() {
                    let config_source = std::fs::read_to_string(&static_config)?;
                    let lines: Vec<_> = config_source.lines().collect();
                    let parsed_config = serde_json::from_str::<serde_json::Value>(&config_source);
                    if let Err(serde_error) = &parsed_config {
                        let line = lines[serde_error.line() - 2];

                        let offset = SourceOffset::from_location(
                            config_source.clone(),
                            serde_error.line() - 1,
                            line.len(),
                        );

                        let error_string = serde_error.to_string();
                        let msgs: Vec<_> = error_string.split(" at ").collect();

                        let diagnostic = ConfigurationError {
                            message: msgs[0].to_string(),
                            src: NamedSource::new("komorebi.json", config_source.clone()),
                            bad_bit: SourceSpan::new(offset, 2),
                        };

                        println!("{:?}", Report::new(diagnostic));
                    }

                    println!(
                        "Found komorebi.json; this file can be passed to the start command with the --config flag\n"
                    );

                    if let Ok(config) = StaticConfig::read(&static_config) {
                        match config.app_specific_configuration_path {
                            None => {
                                println!(
                                    "Application specific configuration file path has not been set. Try running 'komorebic fetch-asc'\n"
                                );
                            }
                            Some(AppSpecificConfigurationPath::Single(path)) if !path.exists() => {
                                println!(
                                    "Application specific configuration file path '{}' does not exist. Try running 'komorebic fetch-asc'\n",
                                    path.display()
                                );
                            }
                            _ => {}
                        }
                    }

                    // Check that this file adheres to the schema static config schema as the last step,
                    // so that more basic errors above can be shown to the error before schema-specific
                    // errors
                    let _ = serde_json::from_str::<StaticConfig>(&config_source)?;

                    let raw = std::fs::read_to_string(static_config)?;
                    StaticConfig::aliases(&raw);
                    StaticConfig::deprecated(&raw);
                    StaticConfig::end_of_life(&raw);

                    if config_whkd.exists() {
                        println!(
                            "Found {}; key bindings will be loaded from here when whkd is started, and you can start it automatically using the --whkd flag\n",
                            config_whkd.to_string_lossy()
                        );
                    } else {
                        println!(
                            "No ~/.config/whkdrc found; you may not be able to control komorebi with your keyboard\n"
                        );
                    }
                } else if config_pwsh.exists() {
                    println!("Found komorebi.ps1; this file will be autoloaded by komorebi\n");
                    if config_whkd.exists() {
                        println!(
                            "Found {}; key bindings will be loaded from here when whkd is started\n",
                            config_whkd.to_string_lossy()
                        );
                    } else {
                        println!(
                            "No ~/.config/whkdrc found; you may not be able to control komorebi with your keyboard\n"
                        );
                    }
                } else if config_ahk.exists() {
                    println!("Found komorebi.ahk; this file will be autoloaded by komorebi\n");
                } else {
                    println!("No komorebi configuration found in {home_display}\n");
                    println!(
                        "If running 'komorebic start --await-configuration', you will manually have to call the following command to begin tiling: komorebic complete-configuration\n"
                    );
                }

                let client = reqwest::blocking::Client::new();

                if let Ok(response) = client
                    .get("https://api.github.com/repos/LGUG2Z/komorebi/releases/latest")
                    .header("User-Agent", "komorebic-version-checker")
                    .send()
                {
                    let version = env!("CARGO_PKG_VERSION");

                    #[derive(Deserialize)]
                    struct Release {
                        tag_name: String,
                    }

                    if let Ok(release) =
                        serde_json::from_str::<Release>(&response.text().unwrap_or_default())
                    {
                        let trimmed = release.tag_name.trim_start_matches("v");
                        if trimmed > version {
                            println!(
                                "An updated version of komorebi is available! https://github.com/LGUG2Z/komorebi/releases/v{trimmed}"
                            );
                        }
                    }
                }
            }
            WindowsCommands::Configuration => {
                let static_config = HOME_DIR.join("komorebi.json");

                if static_config.exists() {
                    println!("{}", static_config.display());
                }
            }
            WindowsCommands::BarConfiguration => {
                let static_config = HOME_DIR.join("komorebi.bar.json");

                if static_config.exists() {
                    println!("{}", static_config.display());
                }
            }
            WindowsCommands::Whkdrc => {
                let whkdrc = WHKD_CONFIG_DIR.join("whkdrc");

                if whkdrc.exists() {
                    println!("{}", whkdrc.display());
                }
            }
            WindowsCommands::DataDirectory => {
                let dir = &*DATA_DIR;
                if dir.exists() {
                    println!("{}", dir.display());
                }
            }
            WindowsCommands::Log => {
                let timestamp = Utc::now().format("%Y-%m-%d").to_string();
                let color_log = std::env::temp_dir().join(format!("komorebi.log.{timestamp}"));
                let file = TailedFile::new(File::open(color_log)?);
                let locked = file.lock();
                #[allow(clippy::significant_drop_in_scrutinee, clippy::lines_filter_map_ok)]
                for line in locked.lines().flatten() {
                    println!("{line}");
                }
            }
            WindowsCommands::ForceFocus => {
                send_message(&SocketMessage::ForceFocus)?;
            }
            WindowsCommands::PromoteFocus => {
                send_message(&SocketMessage::PromoteFocus)?;
            }
            WindowsCommands::Retile => {
                send_message(&SocketMessage::Retile)?;
            }
            WindowsCommands::EagerFocus(args) => {
                send_message(&SocketMessage::EagerFocus(args.exe))?;
            }
            WindowsCommands::InvisibleBorders(args) => {
                send_message(&SocketMessage::InvisibleBorders(Rect {
                    left: args.left,
                    top: args.top,
                    right: args.right,
                    bottom: args.bottom,
                }))?;
            }
            WindowsCommands::ToggleFocusFollowsMouse(args) => {
                send_message(&SocketMessage::ToggleFocusFollowsMouse(args.implementation))?;
            }
            WindowsCommands::ToggleMaximize => {
                send_message(&SocketMessage::ToggleMaximize)?;
            }
            WindowsCommands::WorkspaceCustomLayout(args) => {
                send_message(&SocketMessage::WorkspaceLayoutCustom(
                    args.monitor,
                    args.workspace,
                    args.path,
                ))?;
            }
            WindowsCommands::NamedWorkspaceCustomLayout(args) => {
                send_message(&SocketMessage::NamedWorkspaceLayoutCustom(
                    args.workspace,
                    args.path,
                ))?;
            }
            WindowsCommands::WorkspaceCustomLayoutRule(args) => {
                send_message(&SocketMessage::WorkspaceLayoutCustomRule(
                    args.monitor,
                    args.workspace,
                    args.at_container_count,
                    args.path,
                ))?;
            }
            WindowsCommands::NamedWorkspaceCustomLayoutRule(args) => {
                send_message(&SocketMessage::NamedWorkspaceLayoutCustomRule(
                    args.workspace,
                    args.at_container_count,
                    args.path,
                ))?;
            }
            WindowsCommands::Start(args) => {
                if args.ahk {
                    println!(
                        "EOL: The --ahk flag is now end-of-life and will not receive any further updates or bug fixes"
                    );
                }

                let mut ahk: String = String::from("autohotkey.exe");

                if let Ok(komorebi_ahk_exe) = std::env::var("KOMOREBI_AHK_EXE")
                    && which(&komorebi_ahk_exe).is_ok()
                {
                    ahk = komorebi_ahk_exe;
                }

                if args.whkd && which("whkd").is_err() {
                    bail!(
                        "could not find whkd, please make sure it is installed before using the --whkd flag"
                    );
                }

                if args.masir && which("masir").is_err() {
                    bail!(
                        "could not find masir, please make sure it is installed before using the --masir flag"
                    );
                }

                if args.ahk && which(&ahk).is_err() {
                    bail!(
                        "could not find autohotkey, please make sure it is installed before using the --ahk flag"
                    );
                }

                let mut buf: PathBuf;

                // The komorebi.ps1 shim will only exist in the Path if installed by Scoop
                let exec =
                    if let Ok(output) = Command::new("where.exe").arg("komorebi.ps1").output() {
                        let stdout = String::from_utf8(output.stdout)?;
                        match stdout.trim() {
                            "" => None,
                            // It's possible that a komorebi.ps1 config will be in %USERPROFILE% - ignore this
                            stdout if !stdout.contains("scoop") => None,
                            stdout => {
                                buf = PathBuf::from(stdout);
                                buf.pop(); // %USERPROFILE%\scoop\shims
                                buf.pop(); // %USERPROFILE%\scoop
                                buf.push("apps\\komorebi\\current\\komorebi.exe"); //%USERPROFILE%\scoop\komorebi\current\komorebi.exe
                                Some(buf.to_str().ok_or_eyre(
                                    "cannot create a string from the scoop komorebi path",
                                )?)
                            }
                        }
                    } else {
                        None
                    };

                let mut flags = vec![];
                if let Some(config) = &args.config {
                    if !config.is_file() {
                        bail!("could not find file: {}", config.display());
                    }

                    let config = dunce::simplified(config);
                    flags.push(format!("'--config=\"{}\"'", config.display()));
                }

                if args.ffm {
                    flags.push("'--ffm'".to_string());
                }

                if args.await_configuration {
                    flags.push("'--await-configuration'".to_string());
                }

                if let Some(port) = args.tcp_port {
                    flags.push(format!("'--tcp-port={port}'"));
                }

                if args.clean_state {
                    flags.push("'--clean-state'".to_string());
                }

                let exec = exec.unwrap_or("komorebi.exe");
                let script = if flags.is_empty() {
                    format!("Start-Process '{exec}' -WindowStyle hidden",)
                } else {
                    let argument_list = flags.join(",");
                    format!(
                        "Start-Process '{exec}' -ArgumentList {argument_list} -WindowStyle hidden",
                    )
                };

                let mut launcher = WindowsLauncher {
                    script,
                    config: args.config.clone(),
                    system: sysinfo::System::new_all(),
                };

                if !launcher::start(&mut launcher)? {
                    return Ok(());
                }

                if args.whkd {
                    let script = r"
if (!(Get-Process whkd -ErrorAction SilentlyContinue))
{
  Start-Process whkd -WindowStyle hidden
}
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                if args.ahk {
                    let config_ahk = HOME_DIR.join("komorebi.ahk");
                    let config_ahk = dunce::simplified(&config_ahk);

                    let script = format!(
                        r#"
  Start-Process '"{ahk}"' '"{config}"' -WindowStyle hidden
                "#,
                        config = config_ahk.display()
                    );

                    match powershell_script::run(&script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                let static_config = args.config.clone().or_else(|| {
                    let komorebi_json = HOME_DIR.join("komorebi.json");
                    komorebi_json.is_file().then_some(komorebi_json)
                });

                if args.bar {
                    let mut fallthrough = false;
                    match static_config {
                        None => {
                            fallthrough = true;
                        }
                        Some(ref config) => {
                            let mut config = StaticConfig::read(config)?;
                            if let Some(display_bar_configurations) = &mut config.bar_configurations
                            {
                                for config_file_path in &mut *display_bar_configurations {
                                    let script = format!(
                                        r#"Start-Process "komorebi-bar" '"--config" "{}"' -WindowStyle hidden"#,
                                        config_file_path.to_string_lossy()
                                    );

                                    match powershell_script::run(&script) {
                                        Ok(_) => {
                                            println!("{script}");
                                        }
                                        Err(error) => {
                                            println!("Error: {error}");
                                        }
                                    }
                                }
                            } else {
                                fallthrough = true;
                            }
                        }
                    }

                    if fallthrough {
                        let script = r"
if (!(Get-Process komorebi-bar -ErrorAction SilentlyContinue))
{
  Start-Process komorebi-bar -WindowStyle hidden
}
                ";
                        match powershell_script::run(script) {
                            Ok(_) => {
                                println!("{script}");
                            }
                            Err(error) => {
                                println!("Error: {error}");
                            }
                        }
                    }
                }

                if args.masir {
                    let script = r"
if (!(Get-Process masir -ErrorAction SilentlyContinue))
{
  Start-Process masir -WindowStyle hidden
}
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                println!("\nThank you for using komorebi!\n");
                println!("# Commercial Use License");
                println!(
                    "* View licensing options https://lgug2z.com/software/komorebi - A commercial use license is required to use komorebi at work"
                );
                println!("\n# Personal Use Sponsorship");
                println!(
                    "* Become a sponsor https://github.com/sponsors/LGUG2Z - $5/month makes a big difference"
                );
                println!(
                    "* Leave a tip https://ko-fi.com/lgug2z - An alternative to GitHub Sponsors"
                );
                println!("\n# Community");
                println!(
                    "* Join the Discord https://discord.gg/mGkn66PHkx - Chat, ask questions, share your desktops"
                );
                println!(
                    "* Subscribe to https://youtube.com/@LGUG2Z - Development videos, feature previews and release overviews"
                );
                println!(
                    "* Explore the Awesome Komorebi list https://github.com/LGUG2Z/awesome-komorebi - Projects in the komorebi ecosystem"
                );
                println!("\n# Documentation");
                println!(
                    "* Read the docs https://lgug2z.github.io/komorebi - Quickly search through all komorebic commands"
                );

                let bar_config = args.config.or_else(|| {
                    let bar_json = HOME_DIR.join("komorebi.bar.json");
                    bar_json.is_file().then_some(bar_json)
                });

                if let Some(config) = &static_config {
                    let raw = std::fs::read_to_string(config)?;
                    StaticConfig::aliases(&raw);
                    StaticConfig::deprecated(&raw);
                    StaticConfig::end_of_life(&raw);
                }

                if bar_config.is_some() {
                    let output = Command::new("komorebi-bar.exe").arg("--aliases").output()?;
                    let stdout = String::from_utf8(output.stdout)?;
                    println!("{stdout}");
                }

                let client = reqwest::blocking::Client::new();

                if let Ok(response) = client
                    .get("https://api.github.com/repos/LGUG2Z/komorebi/releases/latest")
                    .header("User-Agent", "komorebic-version-checker")
                    .send()
                {
                    let version = env!("CARGO_PKG_VERSION");

                    #[derive(Deserialize)]
                    struct Release {
                        tag_name: String,
                    }

                    if let Ok(release) =
                        serde_json::from_str::<Release>(&response.text().unwrap_or_default())
                    {
                        let trimmed = release.tag_name.trim_start_matches("v");
                        if trimmed > version {
                            println!(
                                "An updated version of komorebi is available! https://github.com/LGUG2Z/komorebi/releases/v{trimmed}"
                            );
                        }
                    }
                }
            }
            WindowsCommands::Stop(args) => {
                if args.ahk {
                    println!(
                        "EOL: The --ahk flag is now end-of-life and will not receive any further updates or bug fixes"
                    );
                }

                if args.whkd {
                    let script = r"
Stop-Process -Name:whkd -ErrorAction SilentlyContinue
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                if args.bar {
                    let script = r"
Stop-Process -Name:komorebi-bar -ErrorAction SilentlyContinue
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                if args.masir {
                    let script = r"
Stop-Process -Name:masir -ErrorAction SilentlyContinue
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                if args.ahk {
                    let script = r#"
if (Get-Command Get-CimInstance -ErrorAction SilentlyContinue) {
    (Get-CimInstance Win32_Process | Where-Object {
        ($_.CommandLine -like '*komorebi.ahk"') -and
        ($_.Name -in @('AutoHotkey.exe', 'AutoHotkey64.exe', 'AutoHotkey32.exe', 'AutoHotkeyUX.exe'))
    } | Select-Object -First 1) | ForEach-Object {
        Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
    }
} else {
    (Get-WmiObject Win32_Process | Where-Object {
        ($_.CommandLine -like '*komorebi.ahk"') -and
        ($_.Name -in @('AutoHotkey.exe', 'AutoHotkey64.exe', 'AutoHotkey32.exe', 'AutoHotkeyUX.exe'))
    } | Select-Object -First 1) | ForEach-Object {
        Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
    }
}
"#;

                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                send_message(&launcher::stop_message(args.ignore_restore))?;
                let mut system = sysinfo::System::new_all();
                system.refresh_processes(ProcessesToUpdate::All, true);

                if system.processes_by_name("komorebi.exe".as_ref()).count() >= 1 {
                    println!("komorebi is still running, attempting to force-quit");

                    let script = r"
Stop-Process -Name:komorebi -ErrorAction SilentlyContinue
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");

                            let hwnd_json = DATA_DIR.join("komorebi.hwnd.json");

                            let file = File::open(hwnd_json)?;
                            let reader = BufReader::new(file);
                            let hwnds: Vec<isize> = serde_json::from_reader(reader)?;

                            for hwnd in hwnds {
                                restore_window(hwnd);
                            }
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }
            }
            WindowsCommands::Kill(args) => {
                if args.ahk {
                    println!(
                        "EOL: The --ahk flag is now end-of-life and will not receive any further updates or bug fixes"
                    );
                }

                if args.whkd {
                    let script = r"
Stop-Process -Name:whkd -ErrorAction SilentlyContinue
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                if args.bar {
                    let script = r"
Stop-Process -Name:komorebi-bar -ErrorAction SilentlyContinue
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                if args.masir {
                    let script = r"
Stop-Process -Name:masir -ErrorAction SilentlyContinue
                ";
                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }

                if args.ahk {
                    let script = r#"
if (Get-Command Get-CimInstance -ErrorAction SilentlyContinue) {
    (Get-CimInstance Win32_Process | Where-Object {
        ($_.CommandLine -like '*komorebi.ahk"') -and
        ($_.Name -in @('AutoHotkey.exe', 'AutoHotkey64.exe', 'AutoHotkey32.exe', 'AutoHotkeyUX.exe'))
    } | Select-Object -First 1) | ForEach-Object {
        Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
    }
} else {
    (Get-WmiObject Win32_Process | Where-Object {
        ($_.CommandLine -like '*komorebi.ahk"') -and
        ($_.Name -in @('AutoHotkey.exe', 'AutoHotkey64.exe', 'AutoHotkey32.exe', 'AutoHotkeyUX.exe'))
    } | Select-Object -First 1) | ForEach-Object {
        Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
    }
}
"#;

                    match powershell_script::run(script) {
                        Ok(_) => {
                            println!("{script}");
                        }
                        Err(error) => {
                            println!("Error: {error}");
                        }
                    }
                }
            }
            WindowsCommands::ScrollingLayoutColumns(args) => {
                send_message(&SocketMessage::ScrollingLayoutColumns(args.count))?;
            }
            WindowsCommands::ScrollingLayoutFullWidth => {
                send_message(&SocketMessage::ScrollingLayoutFullWidth)?;
            }
            WindowsCommands::CenterFocus => {
                send_message(&SocketMessage::CenterFocus)?;
            }
            WindowsCommands::Consume(args) => {
                send_message(&SocketMessage::Consume(args.value))?;
            }
            WindowsCommands::Expel => {
                send_message(&SocketMessage::Expel)?;
            }
            WindowsCommands::ToggleColumnStack => {
                send_message(&SocketMessage::ToggleColumnStack)?;
            }
            WindowsCommands::Pan(args) => {
                // a flag that has a default counts as given, so what only goes with --stream
                // cannot be left to the parser to say
                if !args.stream && (args.sensitivity.is_some() || args.reversed || args.paging) {
                    command::<WindowsCommands>(&META)
                        .error(
                            clap::error::ErrorKind::ArgumentConflict,
                            "--sensitivity, --reversed and --paging can only be used with --stream",
                        )
                        .exit();
                }

                if args.stream {
                    pan::run(
                        BufReader::new(io::stdin()),
                        pan::Options {
                            sensitivity: args.sensitivity.unwrap_or(1.0),
                            reversed: args.reversed,
                            paging: args.paging,
                        },
                        &mut pan::Socket::new(|| komorebi_client::Connection::open().map(PanLink)),
                    )?;
                } else if let Some(direction) = args.direction {
                    send_message(&SocketMessage::Pan(direction))?;
                }
            }
            WindowsCommands::LayoutRatios(args) => {
                if args.columns.is_none() && args.rows.is_none() {
                    println!(
                        "No ratios provided, nothing to change. Use --columns or --rows to specify ratios."
                    );
                } else {
                    send_message(&SocketMessage::LayoutRatios(args.columns, args.rows))?;
                }
            }
            WindowsCommands::LoadCustomLayout(args) => {
                send_message(&SocketMessage::ChangeLayoutCustom(args.path))?;
            }
            WindowsCommands::MonitorIndexPreference(args) => {
                send_message(&SocketMessage::MonitorIndexPreference(
                    args.index_preference,
                    args.left,
                    args.top,
                    args.right,
                    args.bottom,
                ))?;
            }
            WindowsCommands::Gui => {
                Command::new("komorebi-gui").spawn()?;
            }
            WindowsCommands::ToggleShortcuts => {
                let output = Command::new("taskkill")
                    .args(["/F", "/IM", "komorebi-shortcuts.exe"])
                    .output()?;

                if !output.status.success() {
                    Command::new("komorebi-shortcuts.exe").spawn()?;
                }
            }
            WindowsCommands::RestoreWindows => {
                let hwnd_json = DATA_DIR.join("komorebi.hwnd.json");

                let file = File::open(hwnd_json)?;
                let reader = BufReader::new(file);
                let hwnds: Vec<isize> = serde_json::from_reader(reader)?;

                for hwnd in hwnds {
                    restore_window(hwnd);
                }
            }
            WindowsCommands::FocusFollowsMouse(args) => {
                send_message(&SocketMessage::FocusFollowsMouse(
                    args.implementation,
                    args.boolean_state.into(),
                ))?;
            }
            WindowsCommands::ReloadConfiguration => {
                send_message(&SocketMessage::ReloadConfiguration)?;
            }
            WindowsCommands::WatchConfiguration(args) => {
                send_message(&SocketMessage::WatchConfiguration(args.value.into()))?;
            }
            WindowsCommands::CompleteConfiguration => {
                send_message(&SocketMessage::CompleteConfiguration)?;
            }
            WindowsCommands::IdentifyObjectNameChangeApplication(target) => {
                send_message(&SocketMessage::IdentifyObjectNameChangeApplication(
                    target.identifier,
                    target.id,
                ))?;
            }
            WindowsCommands::IdentifyTrayApplication(target) => {
                send_message(&SocketMessage::IdentifyTrayApplication(
                    target.identifier,
                    target.id,
                ))?;
            }
            WindowsCommands::IdentifyLayeredApplication(target) => {
                send_message(&SocketMessage::IdentifyLayeredApplication(
                    target.identifier,
                    target.id,
                ))?;
            }
            WindowsCommands::RemoveTitleBar(target) => {
                match target.identifier {
                    ApplicationIdentifier::Exe => {}
                    _ => {
                        bail!("this command requires applications to be identified by their exe");
                    }
                }

                send_message(&SocketMessage::RemoveTitleBar(target.identifier, target.id))?;
            }
            WindowsCommands::ToggleTitleBars => {
                send_message(&SocketMessage::ToggleTitleBars)?;
            }
            WindowsCommands::SubscribePipe(args) => {
                send_message(&SocketMessage::AddSubscriberPipe(args.named_pipe))?;
            }
            WindowsCommands::UnsubscribePipe(args) => {
                send_message(&SocketMessage::RemoveSubscriberPipe(args.named_pipe))?;
            }
            WindowsCommands::BorderColour(args) => {
                send_message(&SocketMessage::BorderColour(
                    args.window_kind,
                    args.r,
                    args.g,
                    args.b,
                ))?;
            }
            WindowsCommands::BorderStyle(args) => {
                send_message(&SocketMessage::BorderStyle(args.style))?;
            }
            WindowsCommands::BorderImplementation(args) => {
                send_message(&SocketMessage::BorderImplementation(args.style))?;
            }
            WindowsCommands::StackbarMode(args) => {
                send_message(&SocketMessage::StackbarMode(args.mode))?;
            }
            WindowsCommands::Transparency(args) => {
                send_message(&SocketMessage::Transparency(args.boolean_state.into()))?;
            }
            WindowsCommands::TransparencyAlpha(args) => {
                send_message(&SocketMessage::TransparencyAlpha(args.alpha))?;
            }
            WindowsCommands::ToggleTransparency => {
                send_message(&SocketMessage::ToggleTransparency)?;
            }
            WindowsCommands::Animation(args) => {
                send_message(&SocketMessage::Animation(
                    args.boolean_state.into(),
                    args.animation_type,
                ))?;
            }
            WindowsCommands::AnimationDuration(args) => {
                send_message(&SocketMessage::AnimationDuration(
                    args.duration,
                    args.animation_type,
                ))?;
            }
            WindowsCommands::AnimationStyle(args) => {
                send_message(&SocketMessage::AnimationStyle(
                    args.style,
                    args.animation_type,
                ))?;
            }

            WindowsCommands::ToggleWindowContainerBehaviour => {
                send_message(&SocketMessage::ToggleWindowContainerBehaviour)?;
            }
            WindowsCommands::ToggleWorkspaceWindowContainerBehaviour => {
                send_message(&SocketMessage::ToggleWorkspaceWindowContainerBehaviour)?;
            }
            WindowsCommands::WindowHidingBehaviour(args) => {
                send_message(&SocketMessage::WindowHidingBehaviour(args.value))?;
            }
            WindowsCommands::MonocleFocusBehaviour(args) => {
                send_message(&SocketMessage::MonocleFocusBehaviour(args.behaviour))?;
            }
            WindowsCommands::ToggleMonocleFocusBehaviour => {
                send_message(&SocketMessage::ToggleMonocleFocusBehaviour)?;
            }
            WindowsCommands::UnmanagedWindowOperationBehaviour(args) => {
                send_message(&SocketMessage::UnmanagedWindowOperationBehaviour(
                    args.value,
                ))?;
            }
            WindowsCommands::AhkAppSpecificConfiguration(args) => {
                let content = std::fs::read_to_string(args.path)?;
                let lines = if let Some(override_path) = args.override_path {
                    let override_content = std::fs::read_to_string(override_path)?;

                    ApplicationConfigurationGenerator::generate_ahk(
                        &content,
                        Option::from(override_content.as_str()),
                    )?
                } else {
                    ApplicationConfigurationGenerator::generate_ahk(&content, None)?
                };

                let generated_config = HOME_DIR.join("komorebi.generated.ahk");
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&generated_config)?;

                file.write_all(lines.join("\n").as_bytes())?;

                println!(
                    "\nApplication-specific generated configuration written to {}",
                    generated_config.display()
                );
            }
            WindowsCommands::PwshAppSpecificConfiguration(args) => {
                let content = std::fs::read_to_string(args.path)?;
                let lines = if let Some(override_path) = args.override_path {
                    let override_content = std::fs::read_to_string(override_path)?;

                    ApplicationConfigurationGenerator::generate_pwsh(
                        &content,
                        Option::from(override_content.as_str()),
                    )?
                } else {
                    ApplicationConfigurationGenerator::generate_pwsh(&content, None)?
                };

                let generated_config = HOME_DIR.join("komorebi.generated.ps1");
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&generated_config)?;

                file.write_all(lines.join("\n").as_bytes())?;

                println!(
                    "\nApplication-specific generated configuration written to {}",
                    generated_config.display()
                );
            }
            WindowsCommands::ConvertAppSpecificConfiguration(args) => {
                let content = std::fs::read_to_string(args.path)?;
                let mut asc = ApplicationConfigurationGenerator::load(&content)?;
                asc.sort_by(|a, b| a.name.cmp(&b.name));
                let v2 = ApplicationSpecificConfiguration::from(asc);
                println!("{}", serde_json::to_string_pretty(&v2)?);
            }
            WindowsCommands::FormatAppSpecificConfiguration(args) => {
                let content = std::fs::read_to_string(&args.path)?;
                let formatted_content = ApplicationConfigurationGenerator::format(&content)?;

                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(args.path)?;

                file.write_all(formatted_content.as_bytes())?;

                println!(
                    "File successfully formatted for PRs to https://github.com/LGUG2Z/komorebi-application-specific-configuration"
                );
            }
            WindowsCommands::FetchAppSpecificConfiguration => {
                let content = reqwest::blocking::get("https://raw.githubusercontent.com/LGUG2Z/komorebi-application-specific-configuration/master/applications.json")?
                .text()?;

                let output_file = HOME_DIR.join("applications.json");

                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&output_file)?;

                file.write_all(content.as_bytes())?;

                println!(
                    "Latest version of applications.json from https://github.com/LGUG2Z/komorebi-application-specific-configuration downloaded\n"
                );
                println!(
                    "You can add this to your komorebi.json static configuration file like this: \n\n\"app_specific_configuration_path\": \"{}\"",
                    output_file.display().to_string().replace("\\", "/")
                );
            }
            WindowsCommands::ApplicationSpecificConfigurationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let asc = schemars::schema_for!(ApplicationSpecificConfiguration);
                    let schema = serde_json::to_string_pretty(&asc)?;
                    println!("{schema}");
                }
            }
            WindowsCommands::NotificationSchema => {
                #[cfg(feature = "schemars")]
                {
                    let notification = schemars::schema_for!(komorebi_client::Notification);
                    let schema = serde_json::to_string_pretty(&notification)?;
                    println!("{schema}");
                }
            }
            WindowsCommands::SocketSchema => {
                #[cfg(feature = "schemars")]
                {
                    let socket_message = schemars::schema_for!(SocketMessage);
                    let schema = serde_json::to_string_pretty(&socket_message)?;
                    println!("{schema}");
                }
            }
            WindowsCommands::StaticConfigSchema => {
                #[cfg(feature = "schemars")]
                {
                    let static_config = schemars::schema_for!(StaticConfig);
                    let schema = serde_json::to_string_pretty(&static_config)?;
                    println!("{schema}");
                }
            }
            WindowsCommands::GenerateStaticConfig => {
                print_query(&SocketMessage::GenerateStaticConfig);
            }
            // Deprecated
            #[allow(deprecated)]
            WindowsCommands::AltFocusHack(_)
            | WindowsCommands::IdentifyBorderOverflowApplication(_) => {
                println!("Command deprecated - this is now automatically handled by komorebi! 🎉");
            }
        },
    }

    Ok(())
}

fn show_window(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    // BOOL is returned but does not signify whether or not the operation was succesful
    // https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
    // TODO: error handling
    unsafe {
        let _ = ShowWindow(hwnd, command);
    };
}

fn remove_transparency(hwnd: isize) {
    let _ = komorebi_client::Window::from(hwnd).opaque();
}

fn restore_window(hwnd: isize) {
    show_window(HWND(hwnd as *mut core::ffi::c_void), SW_RESTORE);
    remove_transparency(hwnd);
}
