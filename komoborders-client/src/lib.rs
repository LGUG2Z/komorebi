use serde::Deserialize;
use serde::Serialize;
use std::io::Write;
use std::str::FromStr;
use uds_windows::UnixStream;

const KOMOBORDERS: &str = "komoborders.sock";

pub fn send_message(message: &SocketMessage) -> std::io::Result<()> {
    let socket = dirs::data_local_dir()
        .expect("there is no local data directory")
        .join("komorebi")
        .join(KOMOBORDERS);

    let mut connected = false;
    while !connected {
        if let Ok(mut stream) = UnixStream::connect(&socket) {
            connected = true;
            stream.write_all(serde_json::to_string(message)?.as_bytes())?;
        }
    }

    Ok(())
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum ZOrder {
    Top,
    NoTopMost,
    Bottom,
    TopMost,
}

// impl From<isize> for ZOrder {
//     fn from(value: isize) -> Self {
//         match value {
//             -2 => Self::NoTopMost,
//             -1 => Self::TopMost,
//             0 => Self::Top,
//             1 => Self::Bottom,
//             _ => unimplemented!(),
//         }
//     }
// }

impl Into<isize> for ZOrder {
    fn into(self) -> isize {
        match self {
            ZOrder::Top => 0,
            ZOrder::NoTopMost => -2,
            ZOrder::Bottom => 1,
            ZOrder::TopMost => -1,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum SocketMessage {
    FocusedColour(u32, u32, u32),
    UnfocusedColour(u32, u32, u32),
    MonocleColour(u32, u32, u32),
    StackColour(u32, u32, u32),
    Width(i32),
    Offset(i32),
    ZOrder(ZOrder),
}

impl FromStr for SocketMessage {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
