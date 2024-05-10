#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::redundant_pub_crate,
    clippy::significant_drop_tightening,
    clippy::significant_drop_in_scrutinee
)]

mod border;

use komorebi_client::Rect;
use komorebi_client::UnixListener;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::time::Duration;
use uds_windows::UnixStream;
use windows::Win32::Foundation::HWND;

use crate::border::Border;
use crate::border::BORDER_WIDTH;
use crate::border::FOCUSED;
use crate::border::MONOCLE;
use crate::border::STACK;
use crate::border::UNFOCUSED;
use crate::border::Z_ORDER;
use komorebi::WindowsApi;
use komorebi_client::Rgb;

lazy_static! {
    static ref BORDER_STATE: Mutex<HashMap<String, Border>> = Mutex::new(HashMap::new());
    static ref RECT_STATE: Mutex<HashMap<isize, Rect>> = Mutex::new(HashMap::new());
    static ref FOCUSED_STATE: Mutex<HashMap<isize, FocusKind>> = Mutex::new(HashMap::new());
}

#[derive(Copy, Clone)]
enum FocusKind {
    Unfocused,
    Single,
    Stack,
    Monocle,
}

pub fn read_commands_uds(stream: UnixStream) -> color_eyre::Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    for line in reader.lines() {
        let message = komoborders_client::SocketMessage::from_str(&line?)?;

        match message {
            komoborders_client::SocketMessage::FocusedColour(r, g, b) => FOCUSED.store(
                komorebi::Colour::Rgb(Rgb::new(r, g, b)).into(),
                Ordering::SeqCst,
            ),
            komoborders_client::SocketMessage::UnfocusedColour(r, g, b) => UNFOCUSED.store(
                komorebi::Colour::Rgb(Rgb::new(r, g, b)).into(),
                Ordering::SeqCst,
            ),
            komoborders_client::SocketMessage::MonocleColour(r, g, b) => MONOCLE.store(
                komorebi::Colour::Rgb(Rgb::new(r, g, b)).into(),
                Ordering::SeqCst,
            ),
            komoborders_client::SocketMessage::StackColour(r, g, b) => STACK.store(
                komorebi::Colour::Rgb(Rgb::new(r, g, b)).into(),
                Ordering::SeqCst,
            ),
            komoborders_client::SocketMessage::Width(width) => {
                BORDER_WIDTH.store(width, Ordering::SeqCst)
            }
            komoborders_client::SocketMessage::Offset(offset) => {
                BORDER_WIDTH.store(offset, Ordering::SeqCst)
            }
            komoborders_client::SocketMessage::ZOrder(z_order) => {
                let mut z = Z_ORDER.lock();
                *z = z_order;
            }
        }

        let borders = BORDER_STATE.lock();
        for (_, border) in borders.iter() {
            border.invalidate();
        }
    }

    Ok(())
}

fn main() -> color_eyre::Result<()> {
    WindowsApi::set_process_dpi_awareness_context()?;
    let socket = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi")
        .join("komoborders.sock");

    match std::fs::remove_file(&socket) {
        Ok(()) => {}
        Err(error) => match error.kind() {
            // Doing this because ::exists() doesn't work reliably on Windows via IntelliJ
            ErrorKind::NotFound => {}
            _ => {
                return Err(error.into());
            }
        },
    };

    let listener = UnixListener::bind(&socket)?;
    std::thread::spawn(move || {
        for client in listener.incoming() {
            match client {
                Ok(stream) => match read_commands_uds(stream) {
                    Ok(()) => {
                        println!("processed message");
                    }
                    Err(error) => {
                        println!("{error}");
                    }
                },
                Err(error) => {
                    println!("{error}");
                    break;
                }
            }
        }
    });

    let komorebi = komorebi_client::subscribe("komoborders")?;

    for client in komorebi.incoming() {
        match client {
            Ok(subscription) => {
                let reader = BufReader::new(subscription);

                #[allow(clippy::lines_filter_map_ok)]
                for line in reader.lines().flatten() {
                    if let Ok(notification) =
                        serde_json::from_str::<komorebi_client::Notification>(&line)
                    {
                        let mut borders = BORDER_STATE.lock();
                        // Check the state every time we receive a notification
                        let state = notification.state;

                        for m in state.monitors.elements() {
                            // Only operate on the focused workspace of each monitor
                            if let Some(ws) = m.focused_workspace() {
                                let mut should_proceed = true;

                                // Handle the monocle container separately
                                if let Some(monocle) = ws.monocle_container() {
                                    for (_, border) in borders.iter() {
                                        border.destroy()?;
                                    }

                                    borders.clear();
                                    let border = borders
                                        .entry(monocle.id().clone())
                                        .or_insert_with(|| Border::create(monocle.id()).unwrap());

                                    {
                                        let mut focused = FOCUSED_STATE.lock();
                                        focused.insert(border.hwnd, FocusKind::Monocle);
                                    }

                                    let rect = WindowsApi::window_rect(
                                        monocle.focused_window().unwrap().hwnd(),
                                    )?;

                                    border.update(&rect)?;
                                    should_proceed = false;
                                }

                                if should_proceed {
                                    let is_maximized = WindowsApi::is_zoomed(HWND(
                                        WindowsApi::foreground_window().unwrap_or_default(),
                                    ));

                                    if is_maximized {
                                        for (_, border) in borders.iter() {
                                            border.destroy()?;
                                        }

                                        borders.clear();
                                        should_proceed = false;
                                    }
                                }

                                if should_proceed {
                                    // Destroy any borders not associated with the focused workspace
                                    let container_ids = ws
                                        .containers()
                                        .iter()
                                        .map(|c| c.id().clone())
                                        .collect::<Vec<_>>();

                                    for (id, border) in borders.iter() {
                                        if !container_ids.contains(id) {
                                            border.destroy()?;
                                        }
                                    }

                                    // Remove them from the border map
                                    borders.retain(|k, _| container_ids.contains(k));

                                    for (idx, c) in ws.containers().iter().enumerate() {
                                        // Get the border entry for this container from the map or create one
                                        let border = borders
                                            .entry(c.id().clone())
                                            .or_insert_with(|| Border::create(c.id()).unwrap());

                                        // Update the focused state for all containers on this workspace
                                        {
                                            let mut focused = FOCUSED_STATE.lock();
                                            focused.insert(
                                                border.hwnd,
                                                if idx != ws.focused_container_idx() {
                                                    FocusKind::Unfocused
                                                } else {
                                                    if c.windows().len() > 1 {
                                                        FocusKind::Stack
                                                    } else {
                                                        FocusKind::Single
                                                    }
                                                },
                                            );
                                        }

                                        let rect = WindowsApi::window_rect(
                                            c.focused_window().unwrap().hwnd(),
                                        )?;

                                        border.update(&rect)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(error) => {
                if error.raw_os_error().expect("could not get raw os error") == 109 {
                    while komorebi_client::send_message(
                        &komorebi_client::SocketMessage::AddSubscriberSocket(String::from(
                            "komoborders",
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

    Ok(())
}
