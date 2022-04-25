use crate::widget::BarWidget;
use color_eyre::Report;
use color_eyre::Result;
use komorebi::Notification;
use komorebi::State;
use miow::pipe::NamedPipe;
use parking_lot::Mutex;
use std::io::Read;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::thread::sleep;
use std::time::Duration;

pub struct Workspaces {
    pub enabled: bool,
    pub monitor_idx: usize,
    pub connected: Arc<Mutex<bool>>,
    pub pipe: Arc<Mutex<NamedPipe>>,
    pub state: Arc<Mutex<State>>,
    pub selected: Arc<Mutex<usize>>,
}

impl BarWidget for Workspaces {
    fn output(&mut self) -> Vec<String> {
        let state = self.state.lock();
        let mut workspaces = vec![];

        if let Some(primary_monitor) = state.monitors.elements().get(self.monitor_idx) {
            for (i, workspace) in primary_monitor.workspaces().iter().enumerate() {
                workspaces.push(if let Some(name) = workspace.name() {
                    name.clone()
                } else {
                    format!("{}", i + 1)
                });
            }
        }

        if workspaces.is_empty() || !*self.connected.lock() {
            vec!["komorebi offline".to_string()]
        } else {
            workspaces
        }
    }
}

const PIPE: &str = r#"\\.\pipe\"#;

impl Workspaces {
    pub fn focus(index: usize) -> Result<()> {
        Ok(Command::new("cmd.exe")
            .args([
                "/C",
                "komorebic.exe",
                "focus-workspace",
                &format!("{}", index),
            ])
            .output()
            .map(|_| ())?)
    }

    pub fn init(monitor_idx: usize) -> Result<Self> {
        let name = format!("bar-{}", monitor_idx);
        let pipe = format!("{}\\{}", PIPE, name);

        let mut named_pipe = NamedPipe::new(pipe)?;

        let mut output = Command::new("cmd.exe")
            .args(["/C", "komorebic.exe", "subscribe", &name])
            .output()?;

        while !output.status.success() {
            println!(
                "komorebic.exe failed with error code {:?}, retrying in 5 seconds...",
                output.status.code()
            );

            sleep(Duration::from_secs(5));

            output = Command::new("cmd.exe")
                .args(["/C", "komorebic.exe", "subscribe", &name])
                .output()?;
        }

        named_pipe.connect()?;

        let mut buf = vec![0; 4096];

        let mut bytes_read = named_pipe.read(&mut buf)?;
        let mut data = String::from_utf8(buf[0..bytes_read].to_vec())?;
        while data == "\n" {
            bytes_read = named_pipe.read(&mut buf)?;
            data = String::from_utf8(buf[0..bytes_read].to_vec())?;
        }

        let notification: Notification = serde_json::from_str(&data)?;

        let mut workspaces = Self {
            enabled: true,
            monitor_idx,
            connected: Arc::new(Mutex::new(true)),
            pipe: Arc::new(Mutex::new(named_pipe)),
            state: Arc::new(Mutex::new(notification.state)),
            selected: Arc::new(Mutex::new(0)),
        };

        workspaces.listen()?;
        Ok(workspaces)
    }

    pub fn listen(&mut self) -> Result<()> {
        let state = self.state.clone();
        let pipe = self.pipe.clone();
        let connected = self.connected.clone();
        let selected = self.selected.clone();

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

                        let notification: Notification = serde_json::from_str(&data)?;

                        let mut sl = selected.lock();
                        *sl = notification.state.monitors.elements()[0].focused_workspace_idx();

                        let mut st = state.lock();
                        *st = notification.state;
                    }
                    Err(error) => {
                        // Broken pipe
                        if error.raw_os_error().unwrap() == 109 {
                            {
                                let mut cn = connected.lock();
                                *cn = false;
                            }

                            named_pipe.disconnect()?;

                            let mut output = Command::new("cmd.exe")
                                .args(["/C", "komorebic.exe", "subscribe", "bar"])
                                .output()?;

                            while !output.status.success() {
                                println!(
                                    "komorebic.exe failed with error code {:?}, retrying in 5 seconds...",
                                    output.status.code()
                                );

                                sleep(Duration::from_secs(5));

                                output = Command::new("cmd.exe")
                                    .args(["/C", "komorebic.exe", "subscribe", "bar"])
                                    .output()?;
                            }

                            named_pipe.connect()?;

                            {
                                let mut cn = connected.lock();
                                *cn = true;
                            }
                        } else {
                            return Err(Report::from(error));
                        }
                    }
                }
            }
        });

        Ok(())
    }
}
