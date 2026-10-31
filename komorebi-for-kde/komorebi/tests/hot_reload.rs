//! The daemon and its `komorebi.json`: `komorebi --fake-backend` finds the file in the config home
//! or is given it with `--config`, is configured by it, and applies it again when the file changes.
//!
//! What the daemon has made of the file is read back with `GenerateStaticConfig`. The daemon
//! reloads a file that changed after about two seconds.

use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// The daemon, which is killed when the test ends
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A runtime directory, with a config home and a file in it, that goes when the test ends
struct Home(PathBuf);

impl Home {
    /// /tmp: short enough for a unix socket path on macOS
    fn new(name: &str) -> Self {
        let dir = PathBuf::from(format!("/tmp/komorebi-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        Self(dir)
    }

    fn config_home(&self) -> PathBuf {
        self.0.join("config")
    }

    fn socket(&self) -> PathBuf {
        self.0.join("komorebi").join("komorebi.sock")
    }

    /// Starts the daemon with `args`, the configuration home and the runtime directory of this
    /// test
    fn start(&self, args: &[&str]) -> Daemon {
        Daemon(
            Command::new(env!("CARGO_BIN_EXE_komorebi"))
                .arg("--fake-backend")
                .args(args)
                .env("KOMOREBI_CONFIG_HOME", self.config_home())
                .env("XDG_RUNTIME_DIR", &self.0)
                // the logs go to the home directory, which may not be writable (the nix sandbox)
                .env("HOME", &self.0)
                .env_remove("XDG_DATA_HOME")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("komorebi runs"),
        )
    }

    /// What the daemon says its settings are
    fn settings(&self) -> Option<serde_json::Value> {
        let answer = send(&self.socket(), r#"{"type":"GenerateStaticConfig"}"#).ok()?;
        serde_json::from_str(&answer).ok()
    }

    /// Waits up to 15 seconds for the daemon to say `resize_delta` is what it is given
    fn wait_for_resize_delta(&self, resize_delta: i64) -> bool {
        eventually(|| {
            self.settings()
                .is_some_and(|settings| settings["resize_delta"] == resize_delta)
        })
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Waits up to fifteen seconds for `done`
fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Writes `message` on a new connection to `socket` and returns the answer
fn send(socket: &Path, message: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(message.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;

    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    Ok(answer)
}

fn workspace_names(settings: &serde_json::Value) -> Vec<&str> {
    settings["monitors"][0]["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect()
}

#[test]
fn without_a_file_the_daemon_runs_on_its_defaults() {
    let home = Home::new("hr-none");
    let _daemon = home.start(&[]);

    assert!(home.wait_for_resize_delta(50), "the default delta");
    let settings = home.settings().unwrap();
    // the workspace the monitor started with, which no one named
    assert_eq!(workspace_names(&settings), ["unnamed"]);
}

#[test]
fn the_daemon_reads_komorebi_json_from_the_config_home() {
    let home = Home::new("hr-home");
    std::fs::write(
        home.config_home().join("komorebi.json"),
        r#"{ "resize_delta": 100, "monitors": [ { "workspaces": [ { "name": "web" }, { "name": "code" } ] } ] }"#,
    )
    .unwrap();
    let _daemon = home.start(&[]);

    assert!(home.wait_for_resize_delta(100), "the delta of the file");
    assert_eq!(workspace_names(&home.settings().unwrap()), ["web", "code"]);
}

#[test]
fn the_daemon_follows_the_file_it_is_given_when_the_file_changes() {
    let home = Home::new("hr-follow");
    let file = home.0.join("mine.json");
    std::fs::write(&file, r#"{ "resize_delta": 100 }"#).unwrap();
    // a file in the config home is not the one it was given
    std::fs::write(
        home.config_home().join("komorebi.json"),
        r#"{ "resize_delta": 1 }"#,
    )
    .unwrap();
    let _daemon = home.start(&["--config", file.to_str().unwrap()]);

    assert!(home.wait_for_resize_delta(100), "the file --config gave");

    // written in place
    std::fs::write(
        &file,
        r#"{ "resize_delta": 77, "monitors": [ { "workspaces": [ { "name": "after" } ] } ] }"#,
    )
    .unwrap();
    assert!(home.wait_for_resize_delta(77), "the file written in place");
    assert_eq!(workspace_names(&home.settings().unwrap()), ["after"]);

    // replaced by another file, as an editor does that writes a new file and renames it over
    let replacement = home.0.join("mine.json.new");
    std::fs::write(&replacement, r#"{ "resize_delta": 55 }"#).unwrap();
    std::fs::rename(&replacement, &file).unwrap();
    assert!(home.wait_for_resize_delta(55), "the file replaced");

    // a file that does not parse is no reason to lose what the daemon has
    std::fs::write(&file, r#"{ "resize_delta": "#).unwrap();
    std::thread::sleep(Duration::from_secs(4));
    assert!(
        home.wait_for_resize_delta(55),
        "the daemon still answers, unchanged"
    );
}
