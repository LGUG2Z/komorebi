//! The configuration of the bar without a window: what is accepted, what is skipped on a
//! platform that cannot run a widget (D-coord-1: accepted and skipped with a warning), and the
//! `mouse` messages.

use komorebi_bar_core::config::KomobarConfig;
use komorebi_bar_core::config::MouseMessage;
use komorebi_bar_core::widgets::komorebi_config::KomorebiLayout;
use komorebi_bar_core::widgets::network_config::NetworkConfig;
use komorebi_bar_core::widgets::widget::WidgetConfig;

const WINDOWS: bool = cfg!(target_os = "windows");

fn parse(json: &str) -> KomobarConfig {
    serde_json::from_str(json).expect("the configuration parses")
}

const MIXED: &str = r#"{
  "left_widgets": [
    {"Komorebi": {"workspaces": {"enable": true, "hide_empty_workspaces": false}, "layout": {"enable": true}}},
    {"Keyboard": {"enable": true}}
  ],
  "center_widgets": [{"Media": {"enable": true}}],
  "right_widgets": [
    {"Storage": {"enable": true}},
    {"Time": {"enable": true, "format": "TwentyFourHour"}},
    {"Systray": {"enable": true}},
    {"Cpu": {"enable": false}}
  ],
  "mouse": {"on_middle_click": "echo hi", "on_extra2_click": {"message": {"type": "Retile"}}}
}"#;

fn all(config: &KomobarConfig) -> Vec<&WidgetConfig> {
    config
        .left_widgets
        .iter()
        .chain(config.center_widgets.iter().flatten())
        .chain(config.right_widgets.iter())
        .collect()
}

#[test]
fn windows_only_widgets_are_accepted_on_every_platform() {
    let config = parse(MIXED);
    assert_eq!(all(&config).len(), 7);
    assert!(config.mouse.is_some());
}

#[test]
fn a_widget_the_platform_cannot_run_is_named_and_not_enabled() {
    let config = parse(MIXED);
    let unsupported: Vec<_> = all(&config)
        .into_iter()
        .filter_map(|w| w.unsupported())
        .collect();
    if WINDOWS {
        assert!(unsupported.is_empty());
    } else {
        assert_eq!(unsupported, ["Keyboard", "Media", "Systray"]);
    }

    let enabled: Vec<bool> = all(&config).iter().map(|w| w.enabled()).collect();
    // Komorebi, Keyboard, Media, Storage, Time, Systray, Cpu (disabled)
    let expect = [true, WINDOWS, WINDOWS, true, true, WINDOWS, false];
    assert_eq!(enabled, expect);
}

#[test]
fn a_command_mouse_message_is_a_plain_string() {
    let config = parse(MIXED);
    let mouse = config.mouse.expect("mouse");
    assert!(matches!(mouse.on_middle_click, Some(MouseMessage::Command(ref c)) if c == "echo hi"));
    assert!(matches!(
        mouse.on_extra2_click,
        Some(MouseMessage::Komorebi(_))
    ));
    assert!(mouse.has_command());
}

/// The scenario of the Windows e2e once wrote `{"Command": ...}`; it is not a message.
#[test]
fn a_tagged_command_is_rejected() {
    let bad = r#"{"left_widgets": [], "right_widgets": [],
        "mouse": {"on_middle_click": {"Command": "echo hi"}}}"#;
    assert!(serde_json::from_str::<KomobarConfig>(bad).is_err());
}

#[test]
fn the_network_widget_accepts_the_windows_only_interface_options() {
    let json = r#"{"enable": true, "show_total_activity": false, "show_activity": true,
        "default_interface_options": {"display_type": "Ipv4"}}"#;
    let config: NetworkConfig = serde_json::from_str(json).expect("parses");
    assert!(config.default_interface_options.is_some());
    // the old spellings of the two required keys
    let old =
        r#"{"enable": true, "show_total_data_transmitted": true, "show_network_activity": false}"#;
    assert!(serde_json::from_str::<NetworkConfig>(old).is_ok());
}

#[test]
fn a_layout_is_read_from_its_name() {
    let layout: KomorebiLayout = serde_json::from_str("\"BSP\"").expect("a default layout");
    assert!(matches!(layout, KomorebiLayout::Default(_)));
    assert_eq!(layout.to_string(), "BSP");
    for (name, want) in [
        ("Monocle", "Monocle"),
        ("Floating", "Floating"),
        ("Paused", "Paused"),
    ] {
        let read: KomorebiLayout = serde_json::from_str(&format!("\"{name}\"")).unwrap();
        assert_eq!(read.to_string(), want);
    }
    assert!(serde_json::from_str::<KomorebiLayout>("\"Nonsense\"").is_err());
}

#[test]
fn read_fills_in_the_frame_and_keeps_what_was_set() {
    let dir = std::env::temp_dir().join(format!("bar-core-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bar.json");
    std::fs::write(&path, MIXED).unwrap();
    let config = KomobarConfig::read(&path).expect("reads");
    assert!(config.frame.is_some(), "a missing frame gets the default");
    assert_eq!(config.left_widgets.len(), 2);
    assert!(config.mouse.is_some());
    // anything but json is refused
    std::fs::write(dir.join("bar.toml"), MIXED).unwrap();
    let toml = dir.join("bar.toml");
    assert!(std::panic::catch_unwind(|| KomobarConfig::read(&toml)).is_err());
    std::fs::remove_dir_all(&dir).ok();
}
