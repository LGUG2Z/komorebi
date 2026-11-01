//! [`Monitor`], the workspaces a monitor shows, generic over the platform.
//!
//! The monitor's own data is the same on every platform: its size and work area, the offsets and
//! paddings it gives its workspaces, the ring of workspaces and the wallpaper. What differs is how
//! the platform names a monitor (`Id`, `SerialNumber`), and what else it knows about one (`Extra`,
//! flattened into the monitor's JSON, so that each daemon prints the fields it always did).

use crate::Container;
use crate::Workspace;
use crate::WorkspacePlatform;
use color_eyre::eyre;
use color_eyre::eyre::OptionExt;
use komorebi_collections::ring::Ring;
use komorebi_config::behaviour::FloatingLayerBehaviour;
use komorebi_config::static_config::Wallpaper;
use komorebi_layouts::Layout;
use komorebi_layouts::Rect;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::DeserializeOwned;
use std::collections::VecDeque;
use std::fmt::Debug;

/// What names a monitor of platform `P`, as [`PlatformWindowId`] does for a window
pub type PlatformMonitorId<P> = <P as MonitorPlatform>::Id;

/// What the shared [`Monitor`] needs of a platform, besides what its workspaces need.
pub trait MonitorPlatform: WorkspacePlatform {
    /// What names a monitor: `isize` on Windows (a monitor handle), `u32` on KDE.
    type Id: Copy + PartialEq + Debug;

    /// What stays the same for a monitor across a hot-plug and a restart: a string on KDE, an
    /// optional one on Windows.
    type SerialNumber: Clone + Debug + Default + PartialEq + Serialize + DeserializeOwned;

    /// The fields only this platform has, written flat into the monitor's JSON.
    type Extra: Clone + Debug + PartialEq + Serialize + DeserializeOwned;

    /// The schema of [`MonitorPlatform::Id`], for the JSON schema of a monitor.
    #[cfg(feature = "schemars")]
    type IdSchema: schemars::JsonSchema;

    /// Writes an id as the JSON number it has always been.
    fn serialize_id<S: Serializer>(id: &Self::Id, serializer: S) -> Result<S::Ok, S::Error>;

    /// Reads an id written by [`MonitorPlatform::serialize_id`].
    fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self::Id, D::Error>;

    /// The name the platform last knew for workspace `idx` of this monitor, which a workspace
    /// takes when it is first created (Windows keeps the names in its extra fields).
    fn known_workspace_name(_monitor: &Monitor<Self>, _idx: usize) -> Option<String> {
        None
    }
}

fn serialize_id<P: MonitorPlatform, S: Serializer>(
    id: &P::Id,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    P::serialize_id(id, serializer)
}

fn deserialize_id<'de, P: MonitorPlatform, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<P::Id, D::Error> {
    P::deserialize_id(deserializer)
}

/// The extra fields of a platform that has none.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct NoExtra {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(bound(
    serialize = "P::Window: Serialize, P::SerialNumber: Serialize, P::Extra: Serialize",
    deserialize = "P::Window: Deserialize<'de>, P::SerialNumber: Deserialize<'de>, P::Extra: Deserialize<'de>"
))]
#[cfg_attr(
    feature = "schemars",
    schemars(
        rename = "Monitor",
        bound = "P::Window: schemars::JsonSchema, P::SerialNumber: schemars::JsonSchema, P::Extra: schemars::JsonSchema"
    )
)]
pub struct Monitor<P: MonitorPlatform> {
    #[serde(
        serialize_with = "serialize_id::<P, _>",
        deserialize_with = "deserialize_id::<P, _>"
    )]
    #[cfg_attr(feature = "schemars", schemars(with = "P::IdSchema"))]
    pub id: P::Id,
    pub device: String,
    /// Missing when read means the platform's default (`None` on Windows, where it is optional)
    #[serde(default)]
    pub serial_number_id: P::SerialNumber,
    pub workspaces: Ring<Workspace<P>>,
    pub size: Rect,
    pub work_area_offset: Option<Rect>,
    pub work_area_size: Rect,
    pub window_based_work_area_offset: Option<Rect>,
    pub window_based_work_area_offset_limit: isize,
    pub container_padding: Option<i32>,
    pub workspace_padding: Option<i32>,
    /// The workspace that was in front before the one in front now, which `focus-last-workspace`
    /// shows again
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_focused_workspace: Option<usize>,
    /// The wallpaper this monitor shows, unless the workspace in front has one of its own
    pub wallpaper: Option<Wallpaper>,
    /// Whether a window that opens on a workspace on its floating layer floats, for the
    /// workspaces of this monitor that have no behaviour of their own
    pub floating_layer_behaviour: Option<FloatingLayerBehaviour>,
    /// The fields only this platform has
    #[serde(flatten)]
    pub extra: P::Extra,
}

impl<P: MonitorPlatform> Monitor<P> {
    /// The workspaces of the monitor.
    pub const fn workspaces(&self) -> &VecDeque<Workspace<P>> {
        self.workspaces.elements()
    }

    /// The workspaces of the monitor, mutably.
    pub fn workspaces_mut(&mut self) -> &mut VecDeque<Workspace<P>> {
        self.workspaces.elements_mut()
    }

    /// The workspace in front, if there is one.
    pub fn focused_workspace(&self) -> Option<&Workspace<P>> {
        self.workspaces.focused()
    }

    /// The index of the workspace in front.
    pub const fn focused_workspace_idx(&self) -> usize {
        self.workspaces.focused_idx()
    }

    /// The workspace in front, mutably.
    pub fn focused_workspace_mut(&mut self) -> Option<&mut Workspace<P>> {
        self.workspaces.focused_mut()
    }

    #[tracing::instrument(skip(self))]
    pub fn focus_workspace(&mut self, idx: usize) -> eyre::Result<()> {
        tracing::info!("focusing workspace");

        {
            let workspaces = self.workspaces_mut();

            if workspaces.get(idx).is_none() {
                workspaces.resize(idx + 1, Workspace::default());
            }
            self.last_focused_workspace = Some(self.workspaces.focused_idx());
            self.workspaces.focus(idx);
        }

        // Always set the latest known name when creating the workspace for the first time
        if let Some(name) = P::known_workspace_name(self, idx) {
            self.workspaces_mut()
                .get_mut(idx)
                .ok_or_eyre("there is no workspace")?
                .name = Some(name);
        }

        Ok(())
    }

    pub fn new_workspace_idx(&self) -> usize {
        self.workspaces().len()
    }

    pub fn remove_workspace_by_idx(&mut self, idx: usize) -> Option<Workspace<P>> {
        if idx < self.workspaces().len() {
            return self.workspaces_mut().remove(idx);
        }

        if idx == 0 {
            self.workspaces_mut().push_back(Workspace::default());
        } else {
            self.focus_workspace(idx.saturating_sub(1)).ok()?;
        };

        None
    }

    pub fn add_container(
        &mut self,
        container: Container<P::Window>,
        workspace_idx: Option<usize>,
    ) -> eyre::Result<()> {
        let workspace = if let Some(idx) = workspace_idx {
            self.workspaces_mut()
                .get_mut(idx)
                .ok_or_eyre(format!("there is no workspace at index {}", idx))?
        } else {
            self.focused_workspace_mut()
                .ok_or_eyre("there is no workspace")?
        };

        workspace.add_container_to_back(container);

        Ok(())
    }

    pub fn remove_workspaces(&mut self) -> VecDeque<Workspace<P>> {
        std::mem::take(self.workspaces_mut())
    }

    pub fn ensure_workspace_count(&mut self, ensure_count: usize) {
        if self.workspaces().len() < ensure_count {
            self.workspaces_mut()
                .resize(ensure_count, Workspace::default());
        }
    }

    /// The name of the focused workspace, if it has one
    pub fn focused_workspace_name(&self) -> Option<String> {
        self.focused_workspace()
            .map(|w| w.name.clone())
            .unwrap_or(None)
    }

    /// The layout of the focused workspace, if the monitor has one and its tiling is on
    pub fn focused_workspace_layout(&self) -> Option<Layout> {
        self.focused_workspace().and_then(|workspace| {
            if workspace.tile {
                Some(workspace.layout.clone())
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContainerWindow;
    use crate::WorkspaceWindow;
    use komorebi_layouts::OperationDirection;
    use std::collections::HashMap;

    #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    struct W(u32);

    impl ContainerWindow for W {
        type Id = u32;
        type HideSpot = ();

        fn id(&self) -> u32 {
            self.0
        }

        fn exe_name(&self) -> Option<String> {
            None
        }

        fn hide_window(&self, _spot: ()) -> color_eyre::eyre::Result<()> {
            Ok(())
        }

        fn restore_window(&self) -> color_eyre::eyre::Result<()> {
            Ok(())
        }

        fn hide_stacked(&self, _spot: ()) -> color_eyre::eyre::Result<()> {
            Ok(())
        }

        fn restore_stacked(&self) -> color_eyre::eyre::Result<()> {
            Ok(())
        }
    }

    impl WorkspaceWindow for W {
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

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    struct Extra {
        name: String,
        // a flattened map is read through a buffer that keeps its keys as strings
        #[serde(deserialize_with = "workspace_names")]
        workspace_names: HashMap<usize, String>,
    }

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

    #[derive(Clone, Debug, PartialEq)]
    struct Plat;

    impl WorkspacePlatform for Plat {
        type Window = W;
        type State = ();

        fn foreground_window_id() -> Option<u32> {
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
            _: &Workspace<Self>,
            _: OperationDirection,
        ) -> Option<usize> {
            None
        }

        fn scrolling_insertion_idx(_: &Workspace<Self>) -> Option<usize> {
            None
        }

        fn note_scrolling_departure(_: &Workspace<Self>, _: usize) {}
    }

    impl MonitorPlatform for Plat {
        type Id = isize;
        type SerialNumber = Option<String>;
        type Extra = Extra;
        #[cfg(feature = "schemars")]
        type IdSchema = isize;

        fn serialize_id<S: Serializer>(id: &isize, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_i64(*id as i64)
        }

        fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<isize, D::Error> {
            isize::deserialize(deserializer)
        }

        fn known_workspace_name(monitor: &Monitor<Self>, idx: usize) -> Option<String> {
            monitor.extra.workspace_names.get(&idx).cloned()
        }
    }

    const JSON: &str = r#"{
        "id": 65537,
        "device": "Default_Monitor",
        "serial_number_id": null,
        "size": { "left": 0, "top": 0, "right": 1280, "bottom": 800 },
        "work_area_size": { "left": 0, "top": 0, "right": 1280, "bottom": 752 },
        "work_area_offset": null,
        "window_based_work_area_offset": null,
        "window_based_work_area_offset_limit": 1,
        "workspaces": { "elements": [], "focused": 0 },
        "container_padding": null,
        "workspace_padding": null,
        "wallpaper": null,
        "floating_layer_behaviour": null,
        "name": "DISPLAY1",
        "workspace_names": { "1": "two" }
    }"#;

    #[test]
    fn a_monitor_writes_its_platform_fields_flat_and_its_id_as_a_number() {
        let expected: serde_json::Value = serde_json::from_str(JSON).unwrap();
        let monitor: Monitor<Plat> = serde_json::from_value(expected.clone()).unwrap();

        assert_eq!(monitor.id, 65537);
        assert_eq!(monitor.extra.name, "DISPLAY1");
        assert_eq!(serde_json::to_value(&monitor).unwrap(), expected);
    }

    #[test]
    fn a_workspace_that_is_created_takes_the_name_the_platform_knew() {
        let mut monitor: Monitor<Plat> = serde_json::from_str(JSON).unwrap();

        monitor.ensure_workspace_count(1);
        monitor.focus_workspace(1).unwrap();

        assert_eq!(monitor.workspaces().len(), 2);
        assert_eq!(monitor.focused_workspace_name().as_deref(), Some("two"));
        assert_eq!(monitor.last_focused_workspace, Some(0));
    }

    #[test]
    fn a_monitor_without_extra_fields_serialises_none() {
        #[derive(Clone, Debug, PartialEq)]
        struct Bare;

        impl WorkspacePlatform for Bare {
            type Window = W;
            type State = ();

            fn foreground_window_id() -> Option<u32> {
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
                _: &Workspace<Self>,
                _: OperationDirection,
            ) -> Option<usize> {
                None
            }

            fn scrolling_insertion_idx(_: &Workspace<Self>) -> Option<usize> {
                None
            }

            fn note_scrolling_departure(_: &Workspace<Self>, _: usize) {}
        }

        impl MonitorPlatform for Bare {
            type Id = u32;
            type SerialNumber = String;
            type Extra = NoExtra;
            #[cfg(feature = "schemars")]
            type IdSchema = u32;

            fn serialize_id<S: Serializer>(id: &u32, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u32(*id)
            }

            fn deserialize_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
                u32::deserialize(deserializer)
            }
        }

        let json = r#"{ "id": 7, "device": "Virtual-0", "serial_number_id": "Virtual-0",
            "size": { "left": 0, "top": 0, "right": 1, "bottom": 1 },
            "work_area_size": { "left": 0, "top": 0, "right": 1, "bottom": 1 },
            "work_area_offset": null, "window_based_work_area_offset": null,
            "window_based_work_area_offset_limit": 1,
            "workspaces": { "elements": [], "focused": 0 },
            "container_padding": null, "workspace_padding": null,
            "wallpaper": null, "floating_layer_behaviour": null }"#;
        let expected: serde_json::Value = serde_json::from_str(json).unwrap();
        let monitor: Monitor<Bare> = serde_json::from_value(expected.clone()).unwrap();

        assert_eq!(serde_json::to_value(&monitor).unwrap(), expected);
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn the_schema_of_a_monitor_is_called_monitor_and_has_the_flat_fields() {
        use schemars::JsonSchema;

        assert_eq!(Monitor::<Plat>::schema_name(), "Monitor");
        let schema = serde_json::to_value(schemars::schema_for!(Monitor<Plat>)).unwrap();
        let properties = schema["properties"].as_object().unwrap();

        assert!(properties.contains_key("name"));
        assert!(properties.contains_key("workspaces"));
        assert_eq!(properties["id"]["type"], "integer");
    }
}
