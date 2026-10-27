//! `komorebic` as a person runs it, against a socket the test serves in place of the daemon: what
//! each subcommand writes on the socket, and what it prints.

use std::io::Read;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

/// A runtime directory for one test, short enough for a unix socket path on macOS, with the
/// `komorebi` directory the daemon would make in it
struct Runtime(PathBuf);

impl Runtime {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = PathBuf::from(format!(
            "/tmp/komorebic-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("komorebi")).unwrap();
        Self(dir)
    }

    /// Listens on the daemon's socket, takes one connection, and answers what it reads on it with
    /// `answer`. The thread returns what it read.
    fn serve(&self, answer: &'static str) -> JoinHandle<String> {
        let listener = UnixListener::bind(self.0.join("komorebi").join("komorebi.sock")).unwrap();

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut message = String::new();
            stream.read_to_string(&mut message).unwrap();
            stream.write_all(answer.as_bytes()).unwrap();
            message
        })
    }

    fn komorebic(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_komorebic"))
            .args(args)
            .env("XDG_RUNTIME_DIR", &self.0)
            .output()
            .unwrap()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn state_prints_the_answer_of_the_daemon() {
    let runtime = Runtime::new();
    let daemon = runtime.serve(r#"{"monitors":{"elements":[],"focused":0}}"#);

    let output = runtime.komorebic(&["state"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(daemon.join().unwrap(), r#"{"type":"State"}"#);
    assert_eq!(
        stdout(&output),
        "{\"monitors\":{\"elements\":[],\"focused\":0}}\n"
    );
}

#[test]
fn query_asks_the_question_it_is_given() {
    let runtime = Runtime::new();
    let daemon = runtime.serve("1");

    let output = runtime.komorebic(&["query", "focused-workspace-index"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        daemon.join().unwrap(),
        r#"{"type":"Query","content":"FocusedWorkspaceIndex"}"#
    );
    assert_eq!(stdout(&output), "1\n");
}

#[test]
fn monitor_information_prints_the_answer_of_the_daemon() {
    let runtime = Runtime::new();
    let daemon = runtime.serve("[]");

    let output = runtime.komorebic(&["monitor-info"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(daemon.join().unwrap(), r#"{"type":"MonitorInformation"}"#);
    assert_eq!(stdout(&output), "[]\n");
}

#[test]
fn stop_and_retile_send_their_message_and_print_nothing() {
    for (subcommand, message) in [
        ("stop", r#"{"type":"Stop"}"#),
        ("retile", r#"{"type":"Retile"}"#),
    ] {
        let runtime = Runtime::new();
        let daemon = runtime.serve("");

        let output = runtime.komorebic(&[subcommand]);

        assert!(output.status.success(), "{subcommand}: {output:?}");
        assert_eq!(daemon.join().unwrap(), message, "{subcommand}");
        assert_eq!(stdout(&output), "", "{subcommand}");
    }
}

#[test]
fn subscribe_socket_and_unsubscribe_socket_send_the_name_they_are_given() {
    for (subcommand, message) in [
        (
            "subscribe-socket",
            r#"{"type":"AddSubscriberSocket","content":"bar.sock"}"#,
        ),
        (
            "unsubscribe-socket",
            r#"{"type":"RemoveSubscriberSocket","content":"bar.sock"}"#,
        ),
    ] {
        let runtime = Runtime::new();
        let daemon = runtime.serve("");

        let output = runtime.komorebic(&[subcommand, "bar.sock"]);

        assert!(output.status.success(), "{subcommand}: {output:?}");
        assert_eq!(daemon.join().unwrap(), message, "{subcommand}");
        assert_eq!(stdout(&output), "", "{subcommand}");
    }
}

/// Runs `komorebic` with `args` against a daemon that answers nothing, and checks that it wrote
/// `message` on the socket, printed nothing and exited with status 0
fn sends(args: &[&str], message: &str) {
    let runtime = Runtime::new();
    let daemon = runtime.serve("");

    let output = runtime.komorebic(args);

    assert!(output.status.success(), "{args:?}: {output:?}");
    assert_eq!(daemon.join().unwrap(), message, "{args:?}");
    assert_eq!(stdout(&output), "", "{args:?}");
}

#[test]
fn focus_and_cycle_focus_send_the_direction_they_are_given() {
    sends(
        &["focus", "left"],
        r#"{"type":"FocusWindow","content":"Left"}"#,
    );
    sends(
        &["focus", "down"],
        r#"{"type":"FocusWindow","content":"Down"}"#,
    );
    sends(
        &["cycle-focus", "next"],
        r#"{"type":"CycleFocusWindow","content":"Next"}"#,
    );
    sends(
        &["cycle-focus", "previous"],
        r#"{"type":"CycleFocusWindow","content":"Previous"}"#,
    );
}

#[test]
fn mouse_follows_focus_is_set_with_enable_and_disable_or_toggled() {
    sends(
        &["mouse-follows-focus", "enable"],
        r#"{"type":"MouseFollowsFocus","content":true}"#,
    );
    sends(
        &["mouse-follows-focus", "disable"],
        r#"{"type":"MouseFollowsFocus","content":false}"#,
    );
    sends(
        &["toggle-mouse-follows-focus"],
        r#"{"type":"ToggleMouseFollowsFocus"}"#,
    );
}

#[test]
fn without_a_daemon_a_question_fails() {
    let runtime = Runtime::new();

    let output = runtime.komorebic(&["state"]);

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), "");
}

#[test]
fn data_directory_shows_where_the_socket_is() {
    let runtime = Runtime::new();

    let output = runtime.komorebic(&["data-directory"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        stdout(&output),
        format!("{}\n", runtime.0.join("komorebi").display())
    );
}

#[test]
fn docgen_writes_the_help_of_every_subcommand_in_a_text_fence() {
    let runtime = Runtime::new();
    let docs = runtime.0.join("cli");

    let output = runtime.komorebic(&["docgen", "--output", docs.to_str().unwrap()]);

    assert!(output.status.success(), "{output:?}");
    let mut pages = std::fs::read_dir(&docs)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    pages.sort();
    assert_eq!(
        pages,
        [
            "completions.md",
            "cycle-focus.md",
            "data-directory.md",
            "focus.md",
            "log.md",
            "monitor-information.md",
            "mouse-follows-focus.md",
            "query.md",
            "retile.md",
            "start.md",
            "state.md",
            "stop.md",
            "subscribe-socket.md",
            "toggle-mouse-follows-focus.md",
            "unsubscribe-socket.md",
        ]
    );

    let query = std::fs::read_to_string(Path::new(&docs).join("query.md")).unwrap();
    assert!(query.starts_with("```text\n"), "{query}");
    assert!(query.ends_with("\n```\n"), "{query}");
    assert!(query.contains("focused-workspace-index"), "{query}");
}

#[test]
fn docgen_writes_the_same_pages_whatever_the_width_of_the_terminal() {
    let runtime = Runtime::new();
    // without a terminal, clap wraps the help at the width in COLUMNS
    let query_page = |columns: &str| {
        let docs = runtime.0.join(format!("cli-{columns}"));
        let output = Command::new(env!("CARGO_BIN_EXE_komorebic"))
            .args(["docgen", "--output", docs.to_str().unwrap()])
            .env("COLUMNS", columns)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        std::fs::read_to_string(docs.join("query.md")).unwrap()
    };

    let narrow = query_page("40");

    assert_eq!(narrow, query_page("250"));
    // the possible values of a query are wrapped at 100 columns
    assert!(narrow.lines().any(|line| line.len() > 40), "{narrow}");
    assert!(narrow.lines().all(|line| line.len() <= 100), "{narrow}");
}

#[test]
fn completions_are_a_script_that_knows_the_subcommands() {
    let runtime = Runtime::new();

    let output = runtime.komorebic(&["completions", "bash"]);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("monitor-information"));
}
