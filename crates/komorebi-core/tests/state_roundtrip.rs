//! State JSON round trips from real fixtures: `komorebic state` output of the Windows and KDE
//! daemons (copied from `e2e/lib/tests/fixtures`) and a macOS-shaped one, read into the shared
//! `Monitor<P>` with each platform's window, id, serial number and extra fields, written again,
//! and compared as parsed JSON.
//!
//! The platforms here are only shaped like the real ones (what the shared types are generic over);
//! the windows keep the JSON they were read from, because the real windows write what the
//! operating system knows (title, rectangle) at the moment they are serialised.

mod common;

use common::fixtures::*;
use komorebi_config::behaviour::WindowHidingPosition;
use komorebi_core::ContainerWindow;
use komorebi_core::Monitor;
use komorebi_core::MonitorPlatform;
use komorebi_core::NoExtra;
use komorebi_core::WorkspacePlatform;
use komorebi_core::WorkspaceWindow;
use komorebi_core::ring::Ring;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde_json::Value;
use std::collections::HashMap;

/// A platform shaped like Windows, KDE or macOS: its window keeps the JSON it was read from, and
/// is named by the number under `$id_key`.
macro_rules! shaped_platform {
    ($platform:ident, $window:ident, $id_key:literal, $monitor_id:ty, $serial:ty, $extra:ty) => {
        #[derive(Clone, Debug, PartialEq)]
        struct $window {
            id: i64,
            json: Value,
        }

        impl Serialize for $window {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.json.serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $window {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let json = Value::deserialize(deserializer)?;
                let id = json[$id_key]
                    .as_i64()
                    .ok_or_else(|| serde::de::Error::custom(concat!("no ", $id_key)))?;
                Ok(Self { id, json })
            }
        }

        impl ContainerWindow for $window {
            type Id = i64;
            type HideSpot = ();

            fn id(&self) -> i64 {
                self.id
            }

            fn exe_name(&self) -> Option<String> {
                None
            }

            fn hide_window(&self, _: ()) -> color_eyre::eyre::Result<()> {
                Ok(())
            }

            fn restore_window(&self) -> color_eyre::eyre::Result<()> {
                Ok(())
            }

            fn hide_stacked(&self, _: ()) -> color_eyre::eyre::Result<()> {
                Ok(())
            }

            fn restore_stacked(&self) -> color_eyre::eyre::Result<()> {
                Ok(())
            }
        }

        impl WorkspaceWindow for $window {
            fn title(&self) -> Option<String> {
                None
            }

            fn exe(&self) -> Option<String> {
                None
            }

            fn class(&self) -> Option<String> {
                None
            }

            fn path(&self) -> Option<String> {
                None
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        struct $platform;

        impl WorkspacePlatform for $platform {
            type Window = $window;
            type State = ();

            fn foreground_window_id() -> Option<i64> {
                None
            }

            fn cursor_position() -> Option<(i32, i32)> {
                None
            }

            fn should_act_window(
                _: &str,
                _: &str,
                _: &str,
                _: &str,
                _: &[komorebi_config::matching::MatchingRule],
            ) -> Option<komorebi_config::matching::MatchingRule> {
                None
            }

            fn should_act_window_individual(
                _: &str,
                _: &str,
                _: &str,
                _: &str,
                _: &komorebi_config::matching::IdWithIdentifier,
            ) -> bool {
                false
            }

            fn scrolling_idx_for_direction(
                _: &komorebi_core::Workspace<Self>,
                _: komorebi_layouts::OperationDirection,
            ) -> Option<usize> {
                None
            }

            fn scrolling_insertion_idx(_: &komorebi_core::Workspace<Self>) -> Option<usize> {
                None
            }

            fn note_scrolling_departure(_: &komorebi_core::Workspace<Self>, _: usize) {}
        }

        impl MonitorPlatform for $platform {
            type Id = $monitor_id;
            type SerialNumber = $serial;
            type Extra = $extra;
            #[cfg(feature = "schemars")]
            type IdSchema = $monitor_id;

            fn serialize_id<S: Serializer>(
                id: &$monitor_id,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                serializer.serialize_i64(*id as i64)
            }

            fn deserialize_id<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<$monitor_id, D::Error> {
                <$monitor_id>::deserialize(deserializer)
            }
        }
    };
}

/// What only a Windows monitor has, as the Windows daemon declares it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WindowsExtra {
    name: String,
    device_id: String,
    #[serde(deserialize_with = "workspace_names")]
    workspace_names: HashMap<usize, String>,
}

/// A flattened map is read through a buffer that keeps its keys as strings.
fn workspace_names<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<usize, String>, D::Error> {
    HashMap::<String, String>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, name)| {
            key.parse()
                .map(|key| (key, name))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

/// What only a macOS monitor has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct MacosExtra {
    window_hiding_position: WindowHidingPosition,
}

shaped_platform!(
    Windows,
    WindowsWindow,
    "hwnd",
    isize,
    Option<String>,
    WindowsExtra
);
shaped_platform!(Kde, KdeWindow, "id", u32, String, NoExtra);
shaped_platform!(Macos, MacosWindow, "id", u32, String, MacosExtra);

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// Reads the monitors of a state file as platform `P`, writes them again.
fn round_trip<P: MonitorPlatform>(state: &Value) -> (Ring<Monitor<P>>, Value)
where
    P::Window: Serialize + for<'de> Deserialize<'de>,
{
    let monitors: Ring<Monitor<P>> = serde_json::from_value(state["monitors"].clone()).unwrap();
    let written = serde_json::to_value(&monitors).unwrap();
    (monitors, written)
}

// ---- Windows ----------------------------------------------------------------------------------

#[test]
fn a_windows_tiled_state_is_written_as_it_was_read() {
    let state = json(KFW_TILED);

    let (monitors, written) = round_trip::<Windows>(&state);

    assert_eq!(written, state["monitors"]);
    let monitor = &monitors.elements()[0];
    assert_eq!(monitor.id, 65537);
    assert_eq!(monitor.serial_number_id, None);
    assert_eq!(monitor.extra.name, "DISPLAY1");
    assert_eq!(monitor.extra.device_id, "Default_Monitor-4&183790ca&0&UID0");
    assert_eq!(monitor.workspaces().len(), 1);
    assert_eq!(monitor.workspaces()[0].containers().len(), 3);
}

#[test]
fn a_windows_stacked_state_is_written_as_it_was_read() {
    let state = json(KFW_STACK);

    let (monitors, written) = round_trip::<Windows>(&state);

    assert_eq!(written, state["monitors"]);
    let stacked = monitors.elements()[0].workspaces()[0]
        .containers()
        .iter()
        .find(|c| c.windows().len() > 1)
        .expect("the fixture has a stack");
    assert_eq!(stacked.windows().len(), 2);
}

#[test]
fn a_windows_state_with_a_floating_window_and_the_floating_layer_is_written_as_it_was_read() {
    let state = json(KFW_FLOAT);

    let (monitors, written) = round_trip::<Windows>(&state);

    assert_eq!(written, state["monitors"]);
    let workspace = &monitors.elements()[0].workspaces()[0];
    assert_eq!(workspace.floating_windows().len(), 1);
    assert_eq!(workspace.layer, komorebi_core::WorkspaceLayer::Floating);
    assert!(workspace.contains_window(workspace.floating_windows()[0].id));
}

// ---- KDE --------------------------------------------------------------------------------------

#[test]
fn a_kde_tiled_state_is_written_as_it_was_read() {
    let state = json(KFDE_TILED);

    let (monitors, written) = round_trip::<Kde>(&state);

    assert_eq!(written, state["monitors"]);
    let monitor = &monitors.elements()[0];
    assert_eq!(monitor.id, 1230296105);
    assert_eq!(monitor.device, "Virtual-0");
    assert_eq!(monitor.serial_number_id, "Virtual-0");
    assert_eq!(monitor.workspaces()[0].containers().len(), 3);
}

#[test]
fn a_kde_stacked_state_is_written_as_it_was_read() {
    let state = json(KFDE_STACK);

    let (monitors, written) = round_trip::<Kde>(&state);

    assert_eq!(written, state["monitors"]);
    let first = &monitors.elements()[0].workspaces()[0].containers()[0];
    assert_eq!(first.windows().len(), 2);
    assert_eq!(first.focused_window_idx(), 1);
}

#[test]
fn a_kde_monitor_has_no_platform_fields_beyond_the_shared_ones() {
    let state = json(KFDE_TILED);
    let (_, written) = round_trip::<Kde>(&state);

    let mut keys: Vec<&str> = written["elements"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();

    assert_eq!(
        keys,
        [
            "container_padding",
            "device",
            "floating_layer_behaviour",
            "id",
            "last_focused_workspace",
            "serial_number_id",
            "size",
            "wallpaper",
            "window_based_work_area_offset",
            "window_based_work_area_offset_limit",
            "work_area_offset",
            "work_area_size",
            "workspace_padding",
            "workspaces",
        ]
    );
}

// ---- macOS ------------------------------------------------------------------------------------

#[test]
fn a_macos_shaped_state_is_written_as_it_was_read_with_the_corner_last() {
    let state = json(MACOS_TILED);

    let (monitors, written) = round_trip::<Macos>(&state);

    assert_eq!(written, state["monitors"]);
    let monitor = &monitors.elements()[0];
    assert_eq!(monitor.id, 1);
    assert_eq!(monitor.serial_number_id, "C02XXXXXXXX");
    assert_eq!(
        monitor.extra.window_hiding_position,
        WindowHidingPosition::BottomLeft
    );
    let keys: Vec<&String> = written["elements"][0].as_object().unwrap().keys().collect();
    assert_eq!(keys.last().unwrap().as_str(), "window_hiding_position");
}

#[test]
fn a_macos_state_cannot_be_read_without_its_corner() {
    let mut state = json(MACOS_TILED);
    state["monitors"]["elements"][0]
        .as_object_mut()
        .unwrap()
        .remove("window_hiding_position");

    assert!(serde_json::from_value::<Ring<Monitor<Macos>>>(state["monitors"].clone()).is_err());
}

// ---- shared -----------------------------------------------------------------------------------

#[test]
fn the_same_workspace_keys_are_written_whatever_the_platform() {
    fn workspace_keys(written: &Value) -> Vec<String> {
        let mut keys: Vec<String> = written["elements"][0]["workspaces"]["elements"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    let windows = workspace_keys(&round_trip::<Windows>(&json(KFW_TILED)).1);
    let kde = workspace_keys(&round_trip::<Kde>(&json(KFDE_TILED)).1);
    let macos = workspace_keys(&round_trip::<Macos>(&json(MACOS_TILED)).1);

    assert_eq!(windows, kde);
    assert_eq!(kde, macos);
}

#[test]
fn a_container_without_locked_is_read_from_a_state_file() {
    let mut state = json(KFDE_TILED);
    for container in
        state["monitors"]["elements"][0]["workspaces"]["elements"][0]["containers"]["elements"]
            .as_array_mut()
            .unwrap()
    {
        container.as_object_mut().unwrap().remove("locked");
    }

    let (monitors, _) = round_trip::<Kde>(&state);

    assert!(
        monitors.elements()[0].workspaces()[0]
            .containers()
            .iter()
            .all(|c| !c.locked)
    );
}

/// Reads a KDE-shaped state with `key` taken out of the object at `level`, to say whether it loads.
fn loads_without(level: &str, key: &str) -> bool {
    let mut state = json(KFDE_TILED);
    let monitor = &mut state["monitors"]["elements"][0];
    let target = match level {
        "monitor" => monitor,
        "workspace" => &mut monitor["workspaces"]["elements"][0],
        _ => &mut monitor["workspaces"]["elements"][0]["containers"]["elements"][0],
    };
    assert!(
        target.as_object_mut().unwrap().remove(key).is_some(),
        "the fixture has no {level} key {key}"
    );

    serde_json::from_value::<Ring<Monitor<Kde>>>(state["monitors"].clone()).is_ok()
}

#[test]
fn an_old_state_file_without_the_optional_keys_still_loads() {
    let optional = [
        ("monitor", "serial_number_id"),
        ("monitor", "work_area_offset"),
        ("monitor", "window_based_work_area_offset"),
        ("monitor", "container_padding"),
        ("monitor", "workspace_padding"),
        ("monitor", "last_focused_workspace"),
        ("monitor", "wallpaper"),
        ("monitor", "floating_layer_behaviour"),
        ("workspace", "name"),
        ("workspace", "monocle_container"),
        ("workspace", "maximized_window"),
        ("workspace", "layout_options"),
        ("workspace", "layout_options_rules"),
        ("workspace", "layout_flip"),
        ("workspace", "workspace_padding"),
        ("workspace", "container_padding"),
        ("workspace", "work_area_offset"),
        ("workspace", "window_container_behaviour"),
        ("workspace", "window_container_behaviour_rules"),
        ("workspace", "float_override"),
        ("workspace", "floating_layer_behaviour"),
        ("workspace", "wallpaper"),
        ("container", "locked"),
    ];

    for (level, key) in optional {
        assert!(
            loads_without(level, key),
            "a {level} without {key} should load"
        );
    }
}

#[test]
fn a_state_file_cannot_be_read_without_the_keys_the_shared_types_require() {
    // characterization: these have no default, so a file from before one of them existed (the
    // layer, the work area offset rules, the window-based offset flag) is not read
    let required = [
        ("monitor", "id"),
        ("monitor", "device"),
        ("monitor", "workspaces"),
        ("monitor", "size"),
        ("monitor", "work_area_size"),
        ("monitor", "window_based_work_area_offset_limit"),
        ("workspace", "containers"),
        ("workspace", "floating_windows"),
        ("workspace", "layout"),
        ("workspace", "layout_rules"),
        ("workspace", "latest_layout"),
        ("workspace", "resize_dimensions"),
        ("workspace", "tile"),
        ("workspace", "work_area_offset_rules"),
        ("workspace", "apply_window_based_work_area_offset"),
        ("workspace", "layer"),
        ("container", "id"),
        ("container", "windows"),
    ];

    for (level, key) in required {
        assert!(
            !loads_without(level, key),
            "a {level} without {key} should not load"
        );
    }
}

#[test]
fn a_workspace_written_after_a_missing_optional_key_has_the_key_again() {
    let mut state = json(KFDE_TILED);
    state["monitors"]["elements"][0]["workspaces"]["elements"][0]
        .as_object_mut()
        .unwrap()
        .remove("layout_options_rules");

    let (_, written) = round_trip::<Kde>(&state);

    assert_eq!(
        written["elements"][0]["workspaces"]["elements"][0]["layout_options_rules"],
        serde_json::json!([])
    );
}

#[test]
fn the_runtime_only_parts_of_a_workspace_are_never_written() {
    let (monitors, written) = round_trip::<Kde>(&json(KFDE_TILED));
    let mut monitors = monitors;
    let workspace = &mut monitors.elements_mut()[0].workspaces_mut()[0];
    workspace.globals.border_width = 9;
    workspace.monocle_container_restore_idx = Some(1);

    let rewritten = serde_json::to_value(&monitors).unwrap();
    let keys = rewritten["elements"][0]["workspaces"]["elements"][0]
        .as_object()
        .unwrap();

    assert!(!keys.contains_key("globals"));
    assert!(!keys.contains_key("state"));
    assert!(!keys.contains_key("scrolling"));
    // a restore index is written once there is one, and not before
    assert!(keys.contains_key("monocle_container_restore_idx"));
    assert!(
        !written["elements"][0]["workspaces"]["elements"][0]
            .as_object()
            .unwrap()
            .contains_key("monocle_container_restore_idx")
    );
}

/// The copies in `common/fixtures.rs` are the files of `e2e/lib/tests/fixtures` when the checkout
/// has them (the nix source of the crate does not).
#[test]
fn the_fixtures_are_the_ones_the_e2e_library_has() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/lib/tests/fixtures");
    if !dir.is_dir() {
        return;
    }

    for (name, copy) in [
        ("kfw-tiled", KFW_TILED),
        ("kfw-stack", KFW_STACK),
        ("kfw-float", KFW_FLOAT),
        ("kfde-tiled", KFDE_TILED),
        ("kfde-stack", KFDE_STACK),
    ] {
        let original = std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap();
        assert_eq!(
            json(&original),
            json(copy),
            "{name}.json changed: update common/fixtures.rs"
        );
    }
}
