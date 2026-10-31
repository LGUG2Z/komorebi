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

    /// Runs `komorebic` with `KOMOREBI_CONFIG_HOME` set to `home`
    fn komorebic_with_config_home(&self, home: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_komorebic"))
            .args(args)
            .env("XDG_RUNTIME_DIR", &self.0)
            .env("KOMOREBI_CONFIG_HOME", home)
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
fn stop_with_ignore_restore_sends_the_message_that_leaves_the_hidden_windows_hidden() {
    let runtime = Runtime::new();
    let daemon = runtime.serve("");

    let output = runtime.komorebic(&["stop", "--ignore-restore"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(daemon.join().unwrap(), r#"{"type":"StopIgnoreRestore"}"#);
    assert_eq!(stdout(&output), "");
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
    sends(
        &["toggle-workspace-layer"],
        r#"{"type":"ToggleWorkspaceLayer"}"#,
    );
    sends(&["toggle-lock"], r#"{"type":"ToggleLock"}"#);
    sends(
        &["preselect-direction", "up"],
        r#"{"type":"PreselectDirection","content":"Up"}"#,
    );
    sends(&["cancel-preselect"], r#"{"type":"CancelPreselect"}"#);
}

#[test]
fn the_stack_subcommands_send_the_direction_or_the_index_they_are_given() {
    sends(
        &["stack", "left"],
        r#"{"type":"StackWindow","content":"Left"}"#,
    );
    sends(
        &["stack", "down"],
        r#"{"type":"StackWindow","content":"Down"}"#,
    );
    sends(&["unstack"], r#"{"type":"UnstackWindow"}"#);
    sends(&["stack-all"], r#"{"type":"StackAll"}"#);
    sends(&["unstack-all"], r#"{"type":"UnstackAll"}"#);
    sends(
        &["cycle-stack", "next"],
        r#"{"type":"CycleStack","content":"Next"}"#,
    );
    sends(
        &["cycle-stack", "previous"],
        r#"{"type":"CycleStack","content":"Previous"}"#,
    );
    sends(
        &["cycle-stack-index", "next"],
        r#"{"type":"CycleStackIndex","content":"Next"}"#,
    );
    sends(
        &["focus-stack-window", "2"],
        r#"{"type":"FocusStackWindow","content":2}"#,
    );
}

#[test]
fn the_subcommands_for_new_windows_send_their_message() {
    sends(
        &["toggle-window-container-behaviour"],
        r#"{"type":"ToggleWindowContainerBehaviour"}"#,
    );
    sends(
        &["toggle-workspace-window-container-behaviour"],
        r#"{"type":"ToggleWorkspaceWindowContainerBehaviour"}"#,
    );
    sends(
        &["toggle-float-override"],
        r#"{"type":"ToggleFloatOverride"}"#,
    );
    sends(
        &["toggle-workspace-float-override"],
        r#"{"type":"ToggleWorkspaceFloatOverride"}"#,
    );
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
fn the_commands_of_the_columns_of_the_scrolling_layout_send_their_message() {
    sends(
        &["scrolling-layout-full-width"],
        r#"{"type":"ScrollingLayoutFullWidth"}"#,
    );
    sends(&["center-focus"], r#"{"type":"CenterFocus"}"#);
    sends(&["expand"], r#"{"type":"Expand"}"#);
    sends(
        &["consume", "right"],
        r#"{"type":"Consume","content":"Right"}"#,
    );
    sends(&["expel"], r#"{"type":"Expel"}"#);
    sends(&["toggle-column-stack"], r#"{"type":"ToggleColumnStack"}"#);
    sends(&["pan", "left"], r#"{"type":"Pan","content":"Left"}"#);
}

#[test]
fn pan_and_consume_want_a_direction() {
    let runtime = Runtime::new();

    for arguments in [&["pan"][..], &["pan", "up"], &["consume"]] {
        let output = runtime.komorebic(arguments);
        assert!(!output.status.success(), "{arguments:?}: {output:?}");
    }
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
        &["named-workspace-padding", "mail", "12"],
        r#"{"type":"NamedWorkspacePadding","content":["mail",12]}"#,
    );
    sends(
        &["named-workspace-container-padding", "mail", "8"],
        r#"{"type":"NamedWorkspaceContainerPadding","content":["mail",8]}"#,
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
fn the_border_subcommands_send_what_they_are_given() {
    sends(&["border", "enable"], r#"{"type":"Border","content":true}"#);
    sends(
        &["border", "disable"],
        r#"{"type":"Border","content":false}"#,
    );
    sends(
        &["active-window-border", "enable"],
        r#"{"type":"Border","content":true}"#,
    );
    sends(
        &["border-colour", "10", "20", "30"],
        r#"{"type":"BorderColour","content":["Single",10,20,30]}"#,
    );
    sends(
        &[
            "border-colour",
            "--window-kind",
            "unfocused-locked",
            "1",
            "2",
            "3",
        ],
        r#"{"type":"BorderColour","content":["UnfocusedLocked",1,2,3]}"#,
    );
    sends(
        &["border-width", "8"],
        r#"{"type":"BorderWidth","content":8}"#,
    );
    sends(
        &["border-offset", "2"],
        r#"{"type":"BorderOffset","content":2}"#,
    );
}

#[test]
fn the_focus_follows_mouse_subcommands_send_what_they_are_given() {
    sends(
        &["focus-follows-mouse", "enable"],
        r#"{"type":"FocusFollowsMouse","content":["Windows",true]}"#,
    );
    sends(
        &[
            "focus-follows-mouse",
            "--implementation",
            "komorebi",
            "disable",
        ],
        r#"{"type":"FocusFollowsMouse","content":["Komorebi",false]}"#,
    );
    sends(
        &["toggle-focus-follows-mouse"],
        r#"{"type":"ToggleFocusFollowsMouse","content":"Windows"}"#,
    );
}

#[test]
fn the_transparency_subcommands_send_what_they_are_given() {
    sends(
        &["transparency", "enable"],
        r#"{"type":"Transparency","content":true}"#,
    );
    sends(
        &["transparency", "disable"],
        r#"{"type":"Transparency","content":false}"#,
    );
    sends(
        &["transparency-alpha", "150"],
        r#"{"type":"TransparencyAlpha","content":150}"#,
    );
    sends(&["toggle-transparency"], r#"{"type":"ToggleTransparency"}"#);
}

#[test]
fn the_animation_subcommands_send_what_they_are_given() {
    sends(
        &["animation", "enable"],
        r#"{"type":"Animation","content":[true,null]}"#,
    );
    sends(
        &["animation", "disable", "--animation-type", "movement"],
        r#"{"type":"Animation","content":[false,"movement"]}"#,
    );
    sends(
        &["animation-duration", "120"],
        r#"{"type":"AnimationDuration","content":[120,null]}"#,
    );
    sends(
        &["animation-duration", "500", "-a", "movement"],
        r#"{"type":"AnimationDuration","content":[500,"movement"]}"#,
    );
    sends(
        &["animation-style", "--style", "ease-in-out-sine"],
        r#"{"type":"AnimationStyle","content":["EaseInOutSine",null]}"#,
    );
    sends(
        &["animation-style", "-a", "movement"],
        r#"{"type":"AnimationStyle","content":["Linear","movement"]}"#,
    );
    sends(
        &["animation-fps", "30"],
        r#"{"type":"AnimationFps","content":30}"#,
    );
}

#[test]
fn visible_windows_prints_the_answer_of_the_daemon() {
    let runtime = Runtime::new();
    let daemon = runtime.serve("{}");

    let output = runtime.komorebic(&["visible-windows"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(daemon.join().unwrap(), r#"{"type":"VisibleWindows"}"#);
    assert_eq!(stdout(&output), "{}\n");
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
fn the_monocle_subcommands_send_their_message() {
    sends(&["toggle-monocle"], r#"{"type":"ToggleMonocle"}"#);
    sends(
        &["monocle-focus-behaviour", "cycle"],
        r#"{"type":"MonocleFocusBehaviour","content":"Cycle"}"#,
    );
    sends(
        &["monocle-focus-behaviour", "no-op"],
        r#"{"type":"MonocleFocusBehaviour","content":"NoOp"}"#,
    );
    sends(
        &["toggle-monocle-focus-behaviour"],
        r#"{"type":"ToggleMonocleFocusBehaviour"}"#,
    );
}

#[test]
fn toggle_maximize_sends_its_message() {
    sends(&["toggle-maximize"], r#"{"type":"ToggleMaximize"}"#);
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
fn the_other_workspace_subcommands_send_what_they_are_given() {
    sends(
        &["cycle-workspace", "next"],
        r#"{"type":"CycleFocusWorkspace","content":"Next"}"#,
    );
    sends(
        &["cycle-empty-workspace", "previous"],
        r#"{"type":"CycleFocusEmptyWorkspace","content":"Previous"}"#,
    );
    sends(
        &["cycle-move-to-workspace", "next"],
        r#"{"type":"CycleMoveContainerToWorkspace","content":"Next"}"#,
    );
    sends(
        &["cycle-send-to-workspace", "previous"],
        r#"{"type":"CycleSendContainerToWorkspace","content":"Previous"}"#,
    );
    sends(
        &["focus-last-workspace"],
        r#"{"type":"FocusLastWorkspace"}"#,
    );
    sends(
        &["move-to-last-workspace"],
        r#"{"type":"MoveContainerToLastWorkspace"}"#,
    );
    sends(
        &["send-to-last-workspace"],
        r#"{"type":"SendContainerToLastWorkspace"}"#,
    );
    sends(
        &["focus-workspaces", "2"],
        r#"{"type":"FocusWorkspaceNumbers","content":2}"#,
    );
    sends(&["new-workspace"], r#"{"type":"NewWorkspace"}"#);
    sends(&["close-workspace"], r#"{"type":"CloseWorkspace"}"#);
}

#[test]
fn the_named_workspace_subcommands_send_the_names_they_are_given() {
    sends(
        &["workspace-name", "0", "1", "mail"],
        r#"{"type":"WorkspaceName","content":[0,1,"mail"]}"#,
    );
    sends(
        &["focus-named-workspace", "mail"],
        r#"{"type":"FocusNamedWorkspace","content":"mail"}"#,
    );
    sends(
        &["ensure-workspaces", "0", "3"],
        r#"{"type":"EnsureWorkspaces","content":[0,3]}"#,
    );
    sends(
        &["ensure-named-workspaces", "0", "web", "code"],
        r#"{"type":"EnsureNamedWorkspaces","content":[0,["web","code"]]}"#,
    );
    sends(
        &["workspace-layout", "0", "1", "columns"],
        r#"{"type":"WorkspaceLayout","content":[0,1,"Columns"]}"#,
    );
    sends(
        &["named-workspace-layout", "mail", "rows"],
        r#"{"type":"NamedWorkspaceLayout","content":["mail","Rows"]}"#,
    );
    sends(
        &["workspace-tiling", "0", "1", "disable"],
        r#"{"type":"WorkspaceTiling","content":[0,1,false]}"#,
    );
    sends(
        &["named-workspace-tiling", "mail", "enable"],
        r#"{"type":"NamedWorkspaceTiling","content":["mail",true]}"#,
    );
}

#[test]
fn the_layout_rule_subcommands_send_what_they_are_given() {
    sends(
        &["workspace-layout-rule", "0", "1", "3", "columns"],
        r#"{"type":"WorkspaceLayoutRule","content":[0,1,3,"Columns"]}"#,
    );
    sends(
        &["named-workspace-layout-rule", "mail", "2", "rows"],
        r#"{"type":"NamedWorkspaceLayoutRule","content":["mail",2,"Rows"]}"#,
    );
    sends(
        &["clear-workspace-layout-rules", "0", "1"],
        r#"{"type":"ClearWorkspaceLayoutRules","content":[0,1]}"#,
    );
    sends(
        &["clear-named-workspace-layout-rules", "mail"],
        r#"{"type":"ClearNamedWorkspaceLayoutRules","content":"mail"}"#,
    );
}

#[test]
fn generate_static_config_prints_the_answer_of_the_daemon() {
    let runtime = Runtime::new();
    let daemon = runtime.serve(r#"{"resize_delta":50}"#);

    let output = runtime.komorebic(&["generate-static-config"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(daemon.join().unwrap(), r#"{"type":"GenerateStaticConfig"}"#);
    assert_eq!(stdout(&output), "{\"resize_delta\":50}\n");
}

#[test]
fn configuration_shows_the_path_of_komorebi_json_when_there_is_one() {
    let runtime = Runtime::new();
    let home = runtime.0.join("config");
    std::fs::create_dir_all(&home).unwrap();

    // `config` is another name for it
    for name in ["configuration", "config"] {
        let output = runtime.komorebic_with_config_home(&home, &[name]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(stdout(&output), "", "no file, nothing to show");
    }

    std::fs::write(home.join("komorebi.json"), "{}").unwrap();

    for name in ["configuration", "config"] {
        let output = runtime.komorebic_with_config_home(&home, &[name]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            stdout(&output),
            format!("{}\n", home.join("komorebi.json").display())
        );
    }
}

#[test]
fn replace_configuration_sends_the_path_it_is_given_with_the_variables_replaced() {
    let runtime = Runtime::new();
    let daemon = runtime.serve("");

    let output = runtime.komorebic_with_config_home(
        &runtime.0,
        &["replace-configuration", "$KOMOREBI_CONFIG_HOME/other.json"],
    );

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        daemon.join().unwrap(),
        format!(
            r#"{{"type":"ReplaceConfiguration","content":"{}/other.json"}}"#,
            runtime.0.display()
        )
    );
    assert_eq!(stdout(&output), "");
}

/// Runs `komorebic quickstart` in the config home `home`, which answers `answers` to its questions
fn quickstart(runtime: &Runtime, home: &Path, answers: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_komorebic"))
        .arg("quickstart")
        .env("XDG_RUNTIME_DIR", &runtime.0)
        .env("KOMOREBI_CONFIG_HOME", home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(answers.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn quickstart_writes_the_example_files_and_asks_before_it_overwrites_one() {
    let runtime = Runtime::new();
    let home = runtime.0.join("config");
    std::fs::create_dir_all(&home).unwrap();

    let output = quickstart(&runtime, &home, "");
    assert!(output.status.success(), "{output:?}");
    let example = include_str!("../../docs/komorebi.example.json");
    assert_eq!(
        std::fs::read_to_string(home.join("komorebi.json")).unwrap(),
        example
    );
    assert_eq!(
        std::fs::read_to_string(home.join("applications.json")).unwrap(),
        include_str!("../applications.json")
    );
    assert_eq!(
        std::fs::read_to_string(home.join("skhdrc")).unwrap(),
        include_str!("../../docs/skhdrc.sample")
    );
    assert!(stdout(&output).contains("The following example files were written"));
    assert!(stdout(&output).contains("komorebic start"));

    // a file that is there is kept unless the answer is yes
    std::fs::write(home.join("komorebi.json"), "{}").unwrap();
    std::fs::write(home.join("applications.json"), "{}").unwrap();
    let output = quickstart(&runtime, &home, "n\nyes\n");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(home.join("komorebi.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("applications.json")).unwrap(),
        include_str!("../applications.json")
    );
    // the third question has no answer, which is not a yes
    assert_eq!(
        std::fs::read_to_string(home.join("skhdrc")).unwrap(),
        include_str!("../../docs/skhdrc.sample")
    );
    assert!(stdout(&output).contains("Skipping"));
}

#[test]
fn autostart_writes_an_xdg_entry_that_starts_komorebi_and_takes_it_away_again() {
    let runtime = Runtime::new();
    let config_dir = runtime.0.join("xdg");
    let entry = config_dir.join("autostart").join("komorebi.desktop");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_komorebic"))
            .args(args)
            .env("XDG_RUNTIME_DIR", &runtime.0)
            .env("XDG_CONFIG_HOME", &config_dir)
            .output()
            .unwrap()
    };

    let output = run(&["disable-autostart"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("There is no autostart entry"));

    let output = run(&["enable-autostart"]);
    assert!(output.status.success(), "{output:?}");
    let text = std::fs::read_to_string(&entry).unwrap();
    assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
    assert!(text.contains("\nExec=komorebi\n"), "{text}");
    assert!(
        text.contains("\nX-KDE-autostart-after=plasma-workspace\n"),
        "{text}"
    );

    // `--config` is what the daemon is started with
    let output = run(&[
        "enable-autostart",
        "--config",
        "/etc/komorebi/komorebi.json",
    ]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        std::fs::read_to_string(&entry)
            .unwrap()
            .contains("\nExec=komorebi --config \"/etc/komorebi/komorebi.json\"\n")
    );

    let output = run(&["disable-autostart"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).starts_with("Deleted "), "{output:?}");
    assert!(!entry.exists());
}

#[test]
fn a_config_home_that_is_not_a_directory_is_refused() {
    let runtime = Runtime::new();
    let home = runtime.0.join("not-there");

    let output = runtime.komorebic_with_config_home(&home, &["configuration"]);

    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("which is not a valid directory"),
        "{output:?}"
    );
}

/// The JSON `komorebic` prints for a subcommand that needs no daemon
fn printed(args: &[&str]) -> serde_json::Value {
    let runtime = Runtime::new();

    let output = runtime.komorebic(args);

    assert!(output.status.success(), "{args:?}: {output:?}");
    serde_json::from_str(&stdout(&output)).unwrap_or_else(|error| panic!("{args:?}: {error}"))
}

#[test]
fn the_schema_subcommands_print_a_json_schema_without_a_daemon() {
    let configuration = printed(&["static-config-schema"]);
    assert!(configuration["properties"]["resize_delta"].is_object());
    assert!(configuration["$defs"]["MonitorConfig"].is_object());

    // `asc-schema` is another name for it
    let applications = printed(&["application-specific-configuration-schema"]);
    assert_eq!(applications, printed(&["asc-schema"]));
    assert!(applications["$defs"]["AscApplicationRules"]["properties"]["floating"].is_object());

    assert!(printed(&["notification-schema"])["$defs"]["WindowManagerEvent"].is_object());
    assert!(printed(&["socket-schema"])["oneOf"].is_array());
}

#[test]
fn the_schema_files_of_the_repository_are_what_the_subcommands_print() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    for (file, subcommand) in [
        ("schema.json", "static-config-schema"),
        (
            "schema.asc.json",
            "application-specific-configuration-schema",
        ),
    ] {
        let runtime = Runtime::new();
        let printed = stdout(&runtime.komorebic(&[subcommand]));
        let committed = std::fs::read_to_string(root.join(file)).unwrap();

        assert_eq!(printed, committed, "{file} is stale: run `just jsonschema`");
    }
}

#[test]
fn the_monitor_subcommands_send_what_they_are_given() {
    sends(
        &["focus-monitor", "1"],
        r#"{"type":"FocusMonitorNumber","content":1}"#,
    );
    sends(
        &["cycle-monitor", "next"],
        r#"{"type":"CycleFocusMonitor","content":"Next"}"#,
    );
    sends(
        &["focus-monitor-at-cursor"],
        r#"{"type":"FocusMonitorAtCursor"}"#,
    );
    sends(
        &["focus-monitor-workspace", "1", "2"],
        r#"{"type":"FocusMonitorWorkspaceNumber","content":[1,2]}"#,
    );
    sends(
        &["move-to-monitor", "1"],
        r#"{"type":"MoveContainerToMonitorNumber","content":1}"#,
    );
    sends(
        &["send-to-monitor", "0"],
        r#"{"type":"SendContainerToMonitorNumber","content":0}"#,
    );
    sends(
        &["cycle-move-to-monitor", "previous"],
        r#"{"type":"CycleMoveContainerToMonitor","content":"Previous"}"#,
    );
    sends(
        &["cycle-send-to-monitor", "next"],
        r#"{"type":"CycleSendContainerToMonitor","content":"Next"}"#,
    );
    sends(
        &["move-to-monitor-workspace", "1", "0"],
        r#"{"type":"MoveContainerToMonitorWorkspaceNumber","content":[1,0]}"#,
    );
    sends(
        &["send-to-monitor-workspace", "0", "1"],
        r#"{"type":"SendContainerToMonitorWorkspaceNumber","content":[0,1]}"#,
    );
    sends(
        &["move-to-named-workspace", "mail"],
        r#"{"type":"MoveContainerToNamedWorkspace","content":"mail"}"#,
    );
    sends(
        &["send-to-named-workspace", "mail"],
        r#"{"type":"SendContainerToNamedWorkspace","content":"mail"}"#,
    );
    sends(
        &["move-workspace-to-monitor", "1"],
        r#"{"type":"MoveWorkspaceToMonitorNumber","content":1}"#,
    );
    sends(
        &["cycle-move-workspace-to-monitor", "next"],
        r#"{"type":"CycleMoveWorkspaceToMonitor","content":"Next"}"#,
    );
    sends(
        &["swap-workspaces-with-monitor", "1"],
        r#"{"type":"SwapWorkspacesToMonitorNumber","content":1}"#,
    );
    sends(
        &["cross-monitor-move-behaviour", "insert"],
        r#"{"type":"CrossMonitorMoveBehaviour","content":"Insert"}"#,
    );
    sends(
        &["toggle-cross-monitor-move-behaviour"],
        r#"{"type":"ToggleCrossMonitorMoveBehaviour"}"#,
    );
    sends(
        &["display-index-preference", "1", "DP-2"],
        r#"{"type":"DisplayIndexPreference","content":[1,"DP-2"]}"#,
    );
    sends(
        &["eager-focus", "firefox"],
        r#"{"type":"EagerFocus","content":"firefox"}"#,
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
            "animation-duration.md",
            "animation-fps.md",
            "animation-style.md",
            "animation.md",
            "application-specific-configuration-schema.md",
            "border-colour.md",
            "border-offset.md",
            "border-width.md",
            "border.md",
            "cancel-preselect.md",
            "center-focus.md",
            "change-layout.md",
            "check.md",
            "clear-all-workspace-rules.md",
            "clear-named-workspace-layout-rules.md",
            "clear-named-workspace-rules.md",
            "clear-session-float-rules.md",
            "clear-workspace-layout-rules.md",
            "clear-workspace-rules.md",
            "close-workspace.md",
            "close.md",
            "completions.md",
            "configuration.md",
            "consume.md",
            "container-padding.md",
            "cross-monitor-move-behaviour.md",
            "cycle-empty-workspace.md",
            "cycle-focus.md",
            "cycle-layout.md",
            "cycle-monitor.md",
            "cycle-move-to-monitor.md",
            "cycle-move-to-workspace.md",
            "cycle-move-workspace-to-monitor.md",
            "cycle-move.md",
            "cycle-send-to-monitor.md",
            "cycle-send-to-workspace.md",
            "cycle-stack-index.md",
            "cycle-stack.md",
            "cycle-workspace.md",
            "data-directory.md",
            "disable-autostart.md",
            "display-index-preference.md",
            "eager-focus.md",
            "enable-autostart.md",
            "enforce-workspace-rules.md",
            "ensure-named-workspaces.md",
            "ensure-workspaces.md",
            "expand.md",
            "expel.md",
            "export-kde-shortcuts.md",
            "flip-layout.md",
            "focus-last-workspace.md",
            "focus-monitor-at-cursor.md",
            "focus-monitor-workspace.md",
            "focus-monitor.md",
            "focus-named-workspace.md",
            "focus-stack-window.md",
            "focus-workspace.md",
            "focus-workspaces.md",
            "focus.md",
            "focused-workspace-container-padding.md",
            "focused-workspace-padding.md",
            "generate-static-config.md",
            "global-state.md",
            "global-work-area-offset.md",
            "hotkeys-compile.md",
            "hotkeys-release.md",
            "hotkeys-reload.md",
            "hotkeys-state.md",
            "hotkeys.md",
            "ignore-rule.md",
            "initial-named-workspace-rule.md",
            "initial-workspace-rule.md",
            "layout-ratios.md",
            "load-resize.md",
            "log.md",
            "manage-rule.md",
            "manage.md",
            "minimize.md",
            "monitor-information.md",
            "monitor-work-area-offset.md",
            "monocle-focus-behaviour.md",
            "mouse-follows-focus.md",
            "move-to-last-workspace.md",
            "move-to-monitor-workspace.md",
            "move-to-monitor.md",
            "move-to-named-workspace.md",
            "move-to-workspace.md",
            "move-workspace-to-monitor.md",
            "move.md",
            "named-workspace-container-padding.md",
            "named-workspace-layout-rule.md",
            "named-workspace-layout.md",
            "named-workspace-padding.md",
            "named-workspace-rule.md",
            "named-workspace-tiling.md",
            "new-workspace.md",
            "notification-schema.md",
            "pan.md",
            "preselect-direction.md",
            "promote-focus.md",
            "promote-swap.md",
            "promote-window.md",
            "promote.md",
            "query.md",
            "quick-load-resize.md",
            "quick-save-resize.md",
            "quickstart.md",
            "replace-configuration.md",
            "resize-axis.md",
            "resize-delta.md",
            "resize-edge.md",
            "retile.md",
            "save-resize.md",
            "scrolling-layout-columns.md",
            "scrolling-layout-full-width.md",
            "send-to-last-workspace.md",
            "send-to-monitor-workspace.md",
            "send-to-monitor.md",
            "send-to-named-workspace.md",
            "send-to-workspace.md",
            "session-float-rule.md",
            "session-float-rules.md",
            "socket-schema.md",
            "stack-all.md",
            "stack.md",
            "start.md",
            "state.md",
            "static-config-schema.md",
            "stop.md",
            "subscribe-socket.md",
            "swap-workspaces-with-monitor.md",
            "toggle-column-stack.md",
            "toggle-cross-monitor-move-behaviour.md",
            "toggle-float-override.md",
            "toggle-float.md",
            "toggle-lock.md",
            "toggle-maximize.md",
            "toggle-monocle-focus-behaviour.md",
            "toggle-monocle.md",
            "toggle-mouse-follows-focus.md",
            "toggle-pause.md",
            "toggle-tiling.md",
            "toggle-transparency.md",
            "toggle-window-based-work-area-offset.md",
            "toggle-window-container-behaviour.md",
            "toggle-workspace-float-override.md",
            "toggle-workspace-layer.md",
            "toggle-workspace-window-container-behaviour.md",
            "transparency-alpha.md",
            "transparency.md",
            "unmanage.md",
            "unmanaged-window-operation-behaviour.md",
            "unstack-all.md",
            "unstack.md",
            "unsubscribe-socket.md",
            "visible-windows.md",
            "workspace-layout-rule.md",
            "workspace-layout.md",
            "workspace-name.md",
            "workspace-padding.md",
            "workspace-rule.md",
            "workspace-tiling.md",
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
