//! `SocketMessage`, the one message type of every komorebi socket.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str::FromStr;

use color_eyre::eyre;
use komorebi_layouts::Axis;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::OperationDirection;
use komorebi_layouts::PanDirection;
use komorebi_layouts::PanGesture;
use komorebi_layouts::Rect;
use komorebi_layouts::Sizing;
use komorebi_themes::KomorebiTheme;
use serde::Deserialize;
use serde::Serialize;
use strum::Display;

use crate::animation::AnimationStyle;
use crate::animation::prefix::AnimationPrefix;
use crate::pathext::ResolvedPathBuf;
use crate::types::ApplicationIdentifier;
use crate::types::BorderImplementation;
use crate::types::BorderStyle;
use crate::types::FocusFollowsMouseImplementation;
use crate::types::HidingBehaviour;
use crate::types::MonocleFocusBehaviour;
use crate::types::MoveBehaviour;
use crate::types::OperationBehaviour;
use crate::types::StackbarLabel;
use crate::types::StackbarMode;
use crate::types::StateQuery;
use crate::types::SubscribeOptions;
use crate::types::WindowKind;

/// A state of the window manager as the JSON that `ApplyState` carries.
///
/// The daemon that handles `ApplyState` (Windows only) reads it into its own `State` type, which
/// does not live in this crate. On the wire it is exactly that state: the type is transparent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateBlob(pub serde_json::Value);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for StateBlob {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("State")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "The state of the window manager, as `komorebic state` prints it",
            "type": "object"
        })
    }
}

/// A message to a daemon, as it is written on its socket: JSON, with the variant's name in
/// `type` and its arguments, if it has any, in `content`.
///
/// One enum for every platform: `SocketMessage::platforms` says which daemons handle which
/// variant. A daemon answers a variant it does not handle with an error.
///
/// # Example
///
/// ```rust
/// use komorebi_protocol::message::SocketMessage;
/// use komorebi_protocol::types::StateQuery;
///
/// let query = SocketMessage::Query(StateQuery::FocusedWorkspaceIndex);
/// assert_eq!(
///     String::from_utf8(query.as_bytes()?)?,
///     r#"{"type":"Query","content":"FocusedWorkspaceIndex"}"#,
/// );
///
/// let stop: SocketMessage = r#"{"type":"Stop"}"#.parse()?;
/// assert!(matches!(stop, SocketMessage::Stop));
///
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
// serde_as must be before derive
#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum SocketMessage {
    // Window / Container Commands
    FocusWindow(OperationDirection),
    MoveWindow(OperationDirection),
    PreselectDirection(OperationDirection),
    CancelPreselect,
    CycleFocusWindow(CycleDirection),
    CycleMoveWindow(CycleDirection),
    StackWindow(OperationDirection),
    UnstackWindow,
    CycleStack(CycleDirection),
    CycleStackIndex(CycleDirection),
    FocusStackWindow(usize),
    StackAll,
    UnstackAll,
    ResizeWindowEdge(OperationDirection, Sizing),
    ResizeWindowAxis(Axis, Sizing),
    MoveContainerToLastWorkspace,
    SendContainerToLastWorkspace,
    MoveContainerToMonitorNumber(usize),
    CycleMoveContainerToMonitor(CycleDirection),
    MoveContainerToWorkspaceNumber(usize),
    MoveContainerToNamedWorkspace(String),
    CycleMoveContainerToWorkspace(CycleDirection),
    SendContainerToMonitorNumber(usize),
    CycleSendContainerToMonitor(CycleDirection),
    SendContainerToWorkspaceNumber(usize),
    CycleSendContainerToWorkspace(CycleDirection),
    SendContainerToMonitorWorkspaceNumber(usize, usize),
    MoveContainerToMonitorWorkspaceNumber(usize, usize),
    SendContainerToNamedWorkspace(String),
    CycleMoveWorkspaceToMonitor(CycleDirection),
    MoveWorkspaceToMonitorNumber(usize),
    SwapWorkspacesToMonitorNumber(usize),
    ForceFocus,
    Close,
    Minimize,
    Promote,
    PromoteSwap,
    PromoteFocus,
    PromoteWindow(OperationDirection),
    EagerFocus(String),
    LockMonitorWorkspaceContainer(usize, usize, usize),
    UnlockMonitorWorkspaceContainer(usize, usize, usize),
    ToggleLock,
    ToggleFloat,
    ToggleMonocle,
    ToggleMaximize,
    ToggleWindowContainerBehaviour,
    ToggleFloatOverride,
    WindowHidingBehaviour(HidingBehaviour),
    ToggleCrossMonitorMoveBehaviour,
    CrossMonitorMoveBehaviour(MoveBehaviour),
    ToggleMonocleFocusBehaviour,
    MonocleFocusBehaviour(MonocleFocusBehaviour),
    UnmanagedWindowOperationBehaviour(OperationBehaviour),
    // Current Workspace Commands
    ManageFocusedWindow,
    UnmanageFocusedWindow,
    AdjustContainerPadding(Sizing, i32),
    AdjustWorkspacePadding(Sizing, i32),
    ChangeLayout(DefaultLayout),
    CycleLayout(CycleDirection),
    LayoutRatios(Option<Vec<f32>>, Option<Vec<f32>>),
    ScrollingLayoutColumns(NonZeroUsize),
    ScrollingLayoutFullWidth,
    CenterFocus,
    Expand,
    Consume(OperationDirection),
    Expel,
    ToggleColumnStack,
    Pan(PanDirection),
    PanGesture(PanGesture),
    ChangeLayoutCustom(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    FlipLayout(Axis),
    ToggleWorkspaceWindowContainerBehaviour,
    ToggleWorkspaceFloatOverride,
    // Monitor and Workspace Commands
    MonitorIndexPreference(usize, i32, i32, i32, i32),
    DisplayIndexPreference(usize, String),
    EnsureWorkspaces(usize, usize),
    EnsureNamedWorkspaces(usize, Vec<String>),
    NewWorkspace,
    ToggleTiling,
    Stop,
    StopIgnoreRestore,
    TogglePause,
    Retile,
    RetileWithResizeDimensions,
    QuickSave,
    QuickLoad,
    Save(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    Load(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    CycleFocusMonitor(CycleDirection),
    CycleFocusWorkspace(CycleDirection),
    CycleFocusEmptyWorkspace(CycleDirection),
    FocusMonitorNumber(usize),
    FocusMonitorAtCursor,
    FocusLastWorkspace,
    CloseWorkspace,
    FocusWorkspaceNumber(usize),
    FocusWorkspaceNumbers(usize),
    FocusMonitorWorkspaceNumber(usize, usize),
    FocusNamedWorkspace(String),
    ContainerPadding(usize, usize, i32),
    NamedWorkspaceContainerPadding(String, i32),
    FocusedWorkspaceContainerPadding(i32),
    WorkspacePadding(usize, usize, i32),
    NamedWorkspacePadding(String, i32),
    FocusedWorkspacePadding(i32),
    WorkspaceTiling(usize, usize, bool),
    NamedWorkspaceTiling(String, bool),
    WorkspaceName(usize, usize, String),
    WorkspaceLayout(usize, usize, DefaultLayout),
    NamedWorkspaceLayout(String, DefaultLayout),
    WorkspaceLayoutCustom(usize, usize, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    NamedWorkspaceLayoutCustom(String, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    WorkspaceLayoutRule(usize, usize, usize, DefaultLayout),
    NamedWorkspaceLayoutRule(String, usize, DefaultLayout),
    WorkspaceLayoutCustomRule(
        usize,
        usize,
        usize,
        #[serde_as(as = "ResolvedPathBuf")] PathBuf,
    ),
    NamedWorkspaceLayoutCustomRule(String, usize, #[serde_as(as = "ResolvedPathBuf")] PathBuf),
    ClearWorkspaceLayoutRules(usize, usize),
    ClearNamedWorkspaceLayoutRules(String),
    ToggleWorkspaceLayer,
    // Configuration
    ReloadConfiguration,
    ReplaceConfiguration(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    ReloadStaticConfiguration(#[serde_as(as = "ResolvedPathBuf")] PathBuf),
    WatchConfiguration(bool),
    CompleteConfiguration,
    AltFocusHack(bool),
    Theme(Box<KomorebiTheme>),
    Animation(bool, Option<AnimationPrefix>),
    AnimationDuration(u64, Option<AnimationPrefix>),
    AnimationFps(u64),
    AnimationStyle(AnimationStyle, Option<AnimationPrefix>),
    #[serde(alias = "ActiveWindowBorder")]
    Border(bool),
    #[serde(alias = "ActiveWindowBorderColour")]
    BorderColour(WindowKind, u32, u32, u32),
    #[serde(alias = "ActiveWindowBorderStyle")]
    BorderStyle(BorderStyle),
    BorderWidth(i32),
    BorderOffset(i32),
    BorderImplementation(BorderImplementation),
    Transparency(bool),
    ToggleTransparency,
    TransparencyAlpha(u8),
    InvisibleBorders(Rect),
    StackbarMode(StackbarMode),
    StackbarLabel(StackbarLabel),
    StackbarFocusedTextColour(u32, u32, u32),
    StackbarUnfocusedTextColour(u32, u32, u32),
    StackbarBackgroundColour(u32, u32, u32),
    StackbarHeight(i32),
    StackbarTabWidth(i32),
    StackbarFontSize(i32),
    StackbarFontFamily(Option<String>),
    WorkAreaOffset(Rect),
    MonitorWorkAreaOffset(usize, Rect),
    WorkspaceWorkAreaOffset(usize, usize, Rect),
    ToggleWindowBasedWorkAreaOffset,
    ResizeDelta(i32),
    InitialWorkspaceRule(ApplicationIdentifier, String, usize, usize),
    InitialNamedWorkspaceRule(ApplicationIdentifier, String, String),
    WorkspaceRule(ApplicationIdentifier, String, usize, usize),
    NamedWorkspaceRule(ApplicationIdentifier, String, String),
    ClearWorkspaceRules(usize, usize),
    ClearNamedWorkspaceRules(String),
    ClearAllWorkspaceRules,
    EnforceWorkspaceRules,
    SessionFloatRule,
    SessionFloatRules,
    ClearSessionFloatRules,
    #[serde(alias = "FloatRule")]
    IgnoreRule(ApplicationIdentifier, String),
    ManageRule(ApplicationIdentifier, String),
    IdentifyObjectNameChangeApplication(ApplicationIdentifier, String),
    IdentifyTrayApplication(ApplicationIdentifier, String),
    IdentifyLayeredApplication(ApplicationIdentifier, String),
    IdentifyBorderOverflowApplication(ApplicationIdentifier, String),
    State,
    GlobalState,
    VisibleWindows,
    MonitorInformation,
    Query(StateQuery),
    FocusFollowsMouse(FocusFollowsMouseImplementation, bool),
    ToggleFocusFollowsMouse(FocusFollowsMouseImplementation),
    MouseFollowsFocus(bool),
    ToggleMouseFollowsFocus,
    RemoveTitleBar(ApplicationIdentifier, String),
    ToggleTitleBars,
    AddSubscriberSocket(String),
    AddSubscriberSocketWithOptions(String, SubscribeOptions),
    RemoveSubscriberSocket(String),
    AddSubscriberPipe(String),
    RemoveSubscriberPipe(String),
    ApplicationSpecificConfigurationSchema,
    NotificationSchema,
    SocketSchema,
    StaticConfigSchema,
    GenerateStaticConfig,
    DebugWindow(i64),
    // low level commands
    ApplyState(StateBlob),
    // handled by the KDE daemon only (see `platforms`)
    ReloadHotkeys,
    HotkeysState,
}

impl SocketMessage {
    /// Returns the message as JSON, the way a daemon reads it from its socket
    pub fn as_bytes(&self) -> eyre::Result<Vec<u8>> {
        Ok(serde_json::to_string(self)?.as_bytes().to_vec())
    }
}

impl FromStr for SocketMessage {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_blob_is_written_and_read_as_the_json_it_holds() {
        let text = r#"{"monitors":{"elements":[]}}"#;
        let blob: StateBlob = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&blob).unwrap(), text);
    }
}
