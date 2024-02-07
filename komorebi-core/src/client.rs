use crate::SocketMessage;
use color_eyre::Result;
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use std::io::Write;
use std::{
    io::{BufReader, ErrorKind},
    path::PathBuf,
};
use uds_windows::{UnixListener, UnixStream};

lazy_static! {
    static ref DATA_DIR: PathBuf = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi");
}

fn send_message(bytes: &[u8]) -> Result<()> {
    let socket = DATA_DIR.join("komorebi.sock");

    let mut connected = false;
    while !connected {
        if let Ok(mut stream) = UnixStream::connect(&socket) {
            connected = true;
            stream.write_all(bytes)?;
        }
    }

    Ok(())
}
pub trait SendMessage {
    fn send(self) -> Result<()>;
    fn send_receive<T: DeserializeOwned>(self) -> Result<T>;
}
impl SendMessage for SocketMessage {
    fn send(self) -> Result<()> {
        send_message(&self.as_bytes()?)
    }

    fn send_receive<T: DeserializeOwned>(self) -> Result<T> {
        let socket = DATA_DIR.join("komorebic.sock");

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

        self.send()?;

        let listener = UnixListener::bind(socket)?;
        match listener.accept() {
            Ok(incoming) => {
                let reader = BufReader::new(incoming.0);
                return Ok(serde_json::from_reader(reader)?);
            }
            Err(error) => {
                panic!("{}", error);
            }
        }
    }
}
