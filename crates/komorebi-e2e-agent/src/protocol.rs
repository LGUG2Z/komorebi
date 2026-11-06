//! What the agent says to the host, on every platform: one request is one line of JSON,
//! `{"id": 1, "verb": "...", ...}`, answered by one line, `{"id": 1, "ok": ...}` or
//! `{"id": 1, "error": "..."}`. `put_file` and `get_file` move bytes after the line, as the `size`
//! raw bytes that the request or the answer announces, so that a daemon of tens of megabytes is
//! not wrapped in JSON.
//!
//! The verbs every platform has are here: `ping`, `exec`, `spawn`, `status`, `send`, `put_file`
//! and `get_file`. A platform adds its own (the windows of its desktop, its displays) by
//! implementing [`Verbs`].
//!
//! A frame is `{left, top, right, bottom}` where `right` and `bottom` are the width and the
//! height, the shape komorebi itself prints.

use std::collections::HashMap;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::process::Child;
use std::process::Command;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

/// The port the agent listens on, on every platform.
pub const DEFAULT_PORT: u16 = 7878;

/// The programs `spawn` started and that have not been looked at since they ended: the agent keeps
/// them so that `status` can say how one ended, and `send` can write to its standard input.
static CHILDREN: Mutex<Option<HashMap<u32, Child>>> = Mutex::new(None);

fn children<T>(use_them: impl FnOnce(&mut HashMap<u32, Child>) -> T) -> T {
    let mut guard = CHILDREN.lock().unwrap();
    use_them(guard.get_or_insert_with(HashMap::new))
}

/// A request. A verb reads the fields it needs; the others are left at their defaults.
#[derive(Deserialize)]
pub struct Request {
    pub id: Value,
    pub verb: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size: u64,
    /// The environment a program is started with, on top of the agent's own.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// A file that gets what a spawned program prints, standard output and error.
    #[serde(default)]
    pub out: Option<String>,
    /// A process id of a program `spawn` started.
    #[serde(default)]
    pub pid: u32,
    /// A line for the standard input of that program.
    #[serde(default)]
    pub line: String,
    /// A window handle, on a platform that has them (Windows).
    #[serde(default)]
    #[cfg_attr(not(windows), allow(dead_code))]
    pub hwnd: isize,
    /// A window's title, on a platform that finds a window by it (macOS), and a place on the
    /// desktop for `move`.
    #[serde(default)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub title: String,
    #[serde(default)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub x: f64,
    #[serde(default)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub y: f64,
}

/// Why a command is being prepared: it runs to its end, or it keeps running.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Exec,
    Spawn,
}

/// What a platform adds to the protocol.
pub trait Verbs {
    /// Answers a verb of this platform, or `None` if it is not one.
    fn handle(&mut self, request: &Request) -> Option<Result<Value, String>>;

    /// Settings of the platform for a program the agent starts (no console window on Windows, a
    /// standard input that stays open on macOS).
    fn prepare(&self, _command: &mut Command, _mode: Mode) {}
}

pub fn frame_json(left: i64, top: i64, width: i64, height: i64) -> Value {
    json!({"left": left, "top": top, "right": width, "bottom": height})
}

fn answer(stream: &mut TcpStream, id: &Value, result: Result<Value, String>) {
    let line = match result {
        Ok(ok) => json!({"id": id, "ok": ok}),
        Err(error) => json!({"id": id, "error": error}),
    };
    let _ = writeln!(stream, "{line}");
}

fn exec(request: &Request, verbs: &dyn Verbs) -> Result<Value, String> {
    let (program, rest) = request.args.split_first().ok_or("exec wants a program")?;
    let mut command = Command::new(program);
    command.args(rest).envs(&request.env);
    verbs.prepare(&mut command, Mode::Exec);
    let out = command.output().map_err(|e| format!("{program}: {e}"))?;
    Ok(json!({
        "status": out.status.code(),
        "stdout": String::from_utf8_lossy(&out.stdout),
        "stderr": String::from_utf8_lossy(&out.stderr),
    }))
}

/// Starts a program that keeps running, in this session, and returns at once.
fn spawn(request: &Request, verbs: &dyn Verbs) -> Result<Value, String> {
    let (program, rest) = request.args.split_first().ok_or("spawn wants a program")?;
    let mut command = Command::new(program);
    command.args(rest).envs(&request.env);
    verbs.prepare(&mut command, Mode::Spawn);
    if let Some(path) = &request.out {
        let file = std::fs::File::create(path).map_err(|e| format!("{path}: {e}"))?;
        let again = file.try_clone().map_err(|e| format!("{path}: {e}"))?;
        command.stdout(file).stderr(again);
    }
    let child = command.spawn().map_err(|e| format!("{program}: {e}"))?;
    let pid = child.id();
    children(|children| children.insert(pid, child));
    Ok(json!({"pid": pid}))
}

fn status(pid: u32) -> Result<Value, String> {
    children(|children| {
        let child = children
            .get_mut(&pid)
            .ok_or_else(|| format!("{pid} was not started by this agent"))?;
        match child.try_wait() {
            Ok(Some(status)) => Ok(json!({"running": false, "code": status.code()})),
            Ok(None) => Ok(json!({"running": true})),
            Err(e) => Err(e.to_string()),
        }
    })
}

/// Writes a line to the standard input of a program `spawn` started.
fn send(pid: u32, line: &str) -> Result<Value, String> {
    children(|children| {
        let input = children
            .get_mut(&pid)
            .ok_or_else(|| format!("{pid} was not started by this agent"))?
            .stdin
            .as_mut()
            .ok_or_else(|| format!("{pid} has no standard input"))?;
        writeln!(input, "{line}")
            .and_then(|()| input.flush())
            .map(|()| json!(true))
            .map_err(|e| format!("{pid}: {e}"))
    })
}

/// Serves one connection until the host closes it.
pub fn serve(stream: TcpStream, verbs: &mut dyn Verbs) -> std::io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(e) => {
                answer(&mut writer, &Value::Null, Err(format!("bad request: {e}")));
                continue;
            }
        };
        let result = match request.verb.as_str() {
            "ping" => Ok(json!("pong")),
            "exec" => exec(&request, verbs),
            "spawn" => spawn(&request, verbs),
            "status" => status(request.pid),
            "send" => send(request.pid, &request.line),
            "put_file" => {
                let mut bytes = vec![0u8; request.size as usize];
                reader.read_exact(&mut bytes)?;
                std::fs::write(&request.path, bytes)
                    .map(|()| json!(request.size))
                    .map_err(|e| format!("{}: {e}", request.path))
            }
            "get_file" => match std::fs::read(&request.path) {
                Ok(bytes) => {
                    answer(&mut writer, &request.id, Ok(json!({"size": bytes.len()})));
                    writer.write_all(&bytes)?;
                    continue;
                }
                Err(e) => Err(format!("{}: {e}", request.path)),
            },
            other => verbs
                .handle(&request)
                .unwrap_or_else(|| Err(format!("unknown verb {other}"))),
        };
        answer(&mut writer, &request.id, result);
    }
}

/// The port from the environment, or the default.
pub fn port() -> u16 {
    std::env::var("KOMOREBI_E2E_AGENT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}
