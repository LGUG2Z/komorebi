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
fn move_and_cycle_move_send_the_direction_they_are_given() {
    sends(
        &["move", "right"],
        r#"{"type":"MoveWindow","content":"Right"}"#,
    );
    sends(&["move", "up"], r#"{"type":"MoveWindow","content":"Up"}"#);
    sends(
        &["cycle-move", "previous"],
        r#"{"type":"CycleMoveWindow","content":"Previous"}"#,
    );
}

#[test]
fn the_layout_subcommands_send_the_layout_axis_or_direction_they_are_given() {
    sends(
        &["change-layout", "vertical-stack"],
        r#"{"type":"ChangeLayout","content":"VerticalStack"}"#,
    );
    sends(
        &["change-layout", "bsp"],
        r#"{"type":"ChangeLayout","content":"BSP"}"#,
    );
    sends(
        &["cycle-layout", "next"],
        r#"{"type":"CycleLayout","content":"Next"}"#,
    );
    sends(
        &["flip-layout", "horizontal-and-vertical"],
        r#"{"type":"FlipLayout","content":"HorizontalAndVertical"}"#,
    );
    sends(&["toggle-tiling"], r#"{"type":"ToggleTiling"}"#);
    sends(&["toggle-float"], r#"{"type":"ToggleFloat"}"#);
    sends(&["toggle-lock"], r#"{"type":"ToggleLock"}"#);
    sends(
        &["preselect-direction", "up"],
        r#"{"type":"PreselectDirection","content":"Up"}"#,
    );
    sends(&["cancel-preselect"], r#"{"type":"CancelPreselect"}"#);
}

#[test]
fn scrolling_layout_columns_and_layout_ratios_send_what_they_are_given() {
    sends(
        &["scrolling-layout-columns", "3"],
        r#"{"type":"ScrollingLayoutColumns","content":3}"#,
    );
    sends(
        &["layout-ratios", "--columns", "0.6", "0.25", "--rows", "0.5"],
        r#"{"type":"LayoutRatios","content":[[0.6,0.25],[0.5]]}"#,
    );
    sends(
        &["layout-ratios", "--rows", "0.3"],
        r#"{"type":"LayoutRatios","content":[null,[0.3]]}"#,
    );
}

#[test]
fn layout_ratios_without_ratios_sends_nothing_and_says_so() {
    let runtime = Runtime::new();

    let output = runtime.komorebic(&["layout-ratios"]);

    assert!(output.status.success(), "{output:?}");
    assert!(
        stdout(&output).starts_with("No ratios provided"),
        "{output:?}"
    );
}

#[test]
fn scrolling_layout_columns_refuses_zero() {
    let runtime = Runtime::new();

    let output = runtime.komorebic(&["scrolling-layout-columns", "0"]);

    assert!(!output.status.success(), "{output:?}");
}

#[test]
fn the_resize_subcommands_send_what_they_are_given() {
    sends(
        &["resize-edge", "left", "increase"],
        r#"{"type":"ResizeWindowEdge","content":["Left","Increase"]}"#,
    );
    sends(
        &["resize", "down", "decrease"],
        r#"{"type":"ResizeWindowEdge","content":["Down","Decrease"]}"#,
    );
    sends(
        &["resize-axis", "horizontal-and-vertical", "increase"],
        r#"{"type":"ResizeWindowAxis","content":["HorizontalAndVertical","Increase"]}"#,
    );
    sends(
        &["resize-delta", "75"],
        r#"{"type":"ResizeDelta","content":75}"#,
    );
    sends(&["quick-save-resize"], r#"{"type":"QuickSave"}"#);
    sends(&["quick-load"], r#"{"type":"QuickLoad"}"#);
    sends(
        &["save-resize", "/tmp/sizes.json"],
        r#"{"type":"Save","content":"/tmp/sizes.json"}"#,
    );
    sends(
        &["load", "/tmp/sizes.json"],
        r#"{"type":"Load","content":"/tmp/sizes.json"}"#,
    );
}

#[test]
fn the_promote_subcommands_send_their_message() {
    sends(&["promote"], r#"{"type":"Promote"}"#);
    sends(&["promote-swap"], r#"{"type":"PromoteSwap"}"#);
    sends(&["promote-focus"], r#"{"type":"PromoteFocus"}"#);
    sends(
        &["promote-window", "right"],
        r#"{"type":"PromoteWindow","content":"Right"}"#,
    );
}

#[test]
fn the_padding_subcommands_send_the_workspace_and_the_pixels_they_are_given() {
    sends(
        &["focused-workspace-padding", "20"],
        r#"{"type":"FocusedWorkspacePadding","content":20}"#,
    );
    sends(
        &["focused-workspace-container-padding", "10"],
        r#"{"type":"FocusedWorkspaceContainerPadding","content":10}"#,
    );
    sends(
        &["workspace-padding", "0", "1", "12"],
        r#"{"type":"WorkspacePadding","content":[0,1,12]}"#,
    );
    sends(
        &["container-padding", "1", "0", "8"],
        r#"{"type":"ContainerPadding","content":[1,0,8]}"#,
    );
    sends(
        &["adjust-workspace-padding", "increase", "5"],
        r#"{"type":"AdjustWorkspacePadding","content":["Increase",5]}"#,
    );
    sends(
        &["adjust-container-padding", "decrease", "3"],
        r#"{"type":"AdjustContainerPadding","content":["Decrease",3]}"#,
    );
}

#[test]
fn the_offset_subcommands_send_the_offset_they_are_given() {
    sends(
        &["global-work-area-offset", "10", "20", "30", "40"],
        r#"{"type":"WorkAreaOffset","content":{"left":10,"top":20,"right":30,"bottom":40}}"#,
    );
    sends(
        &["monitor-work-area-offset", "1", "10", "20", "30", "40"],
        r#"{"type":"MonitorWorkAreaOffset","content":[1,{"left":10,"top":20,"right":30,"bottom":40}]}"#,
    );
    sends(
        &[
            "workspace-work-area-offset",
            "1",
            "2",
            "10",
            "20",
            "30",
            "40",
        ],
        r#"{"type":"WorkspaceWorkAreaOffset","content":[1,2,{"left":10,"top":20,"right":30,"bottom":40}]}"#,
    );
    sends(
        &["toggle-window-based-work-area-offset"],
        r#"{"type":"ToggleWindowBasedWorkAreaOffset"}"#,
    );
}

#[test]
fn the_rule_subcommands_send_the_application_they_are_given() {
    sends(
        &["ignore-rule", "exe", "steam"],
        r#"{"type":"IgnoreRule","content":["Exe","steam"]}"#,
    );
    sends(
        &["float-rule", "class", "org.kde.kcalc"],
        r#"{"type":"IgnoreRule","content":["Class","org.kde.kcalc"]}"#,
    );
    sends(
        &["manage-rule", "title", "Picture-in-Picture"],
        r#"{"type":"ManageRule","content":["Title","Picture-in-Picture"]}"#,
    );
    sends(&["session-float-rule"], r#"{"type":"SessionFloatRule"}"#);
    sends(
        &["clear-session-float-rules"],
        r#"{"type":"ClearSessionFloatRules"}"#,
    );
}

#[test]
fn global_state_and_session_float_rules_print_the_answer_of_the_daemon() {
    for (args, message) in [
        ("global-state", r#"{"type":"GlobalState"}"#),
        ("session-float-rules", r#"{"type":"SessionFloatRules"}"#),
    ] {
        let runtime = Runtime::new();
        let daemon = runtime.serve("[]");

        let output = runtime.komorebic(&[args]);

        assert!(output.status.success(), "{output:?}");
        assert_eq!(daemon.join().unwrap(), message);
        assert_eq!(stdout(&output), "[]\n");
    }
}

#[test]
fn the_subcommands_about_the_focused_window_and_pausing_send_their_message() {
    sends(&["minimize"], r#"{"type":"Minimize"}"#);
    sends(&["close"], r#"{"type":"Close"}"#);
    sends(&["manage"], r#"{"type":"ManageFocusedWindow"}"#);
    sends(&["unmanage"], r#"{"type":"UnmanageFocusedWindow"}"#);
    sends(&["toggle-pause"], r#"{"type":"TogglePause"}"#);
    sends(
        &["unmanaged-window-operation-behaviour", "no-op"],
        r#"{"type":"UnmanagedWindowOperationBehaviour","content":"NoOp"}"#,
    );
    sends(
        &["unmanaged-window-operation-behaviour", "op"],
        r#"{"type":"UnmanagedWindowOperationBehaviour","content":"Op"}"#,
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
fn the_workspace_subcommands_send_the_index_they_are_given() {
    sends(
        &["focus-workspace", "1"],
        r#"{"type":"FocusWorkspaceNumber","content":1}"#,
    );
    sends(
        &["move-to-workspace", "2"],
        r#"{"type":"MoveContainerToWorkspaceNumber","content":2}"#,
    );
    sends(
        &["send-to-workspace", "0"],
        r#"{"type":"SendContainerToWorkspaceNumber","content":0}"#,
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
            "adjust-container-padding.md",
            "adjust-workspace-padding.md",
            "cancel-preselect.md",
            "change-layout.md",
            "clear-session-float-rules.md",
            "close.md",
            "completions.md",
            "container-padding.md",
            "cycle-focus.md",
            "cycle-layout.md",
            "cycle-move.md",
            "data-directory.md",
            "flip-layout.md",
            "focus-workspace.md",
            "focus.md",
            "focused-workspace-container-padding.md",
            "focused-workspace-padding.md",
            "global-state.md",
            "global-work-area-offset.md",
            "ignore-rule.md",
            "layout-ratios.md",
            "load-resize.md",
            "log.md",
            "manage-rule.md",
            "manage.md",
            "minimize.md",
            "monitor-information.md",
            "monitor-work-area-offset.md",
            "mouse-follows-focus.md",
            "move-to-workspace.md",
            "move.md",
            "preselect-direction.md",
            "promote-focus.md",
            "promote-swap.md",
            "promote-window.md",
            "promote.md",
            "query.md",
            "quick-load-resize.md",
            "quick-save-resize.md",
            "resize-axis.md",
            "resize-delta.md",
            "resize-edge.md",
            "retile.md",
            "save-resize.md",
            "scrolling-layout-columns.md",
            "send-to-workspace.md",
            "session-float-rule.md",
            "session-float-rules.md",
            "start.md",
            "state.md",
            "stop.md",
            "subscribe-socket.md",
            "toggle-float.md",
            "toggle-lock.md",
            "toggle-mouse-follows-focus.md",
            "toggle-pause.md",
            "toggle-tiling.md",
            "toggle-window-based-work-area-offset.md",
            "unmanage.md",
            "unmanaged-window-operation-behaviour.md",
            "unsubscribe-socket.md",
            "workspace-padding.md",
            "workspace-work-area-offset.md",
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
