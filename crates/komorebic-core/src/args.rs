//! The argument blocks the subcommands of `komorebic` are built from.
//!
//! These replace the `gen_*` macros that every `komorebic` used to carry. A block is a plain struct
//! that derives [`clap::Args`]; the ones that repeat a pair of arguments (`monitor` and `workspace`)
//! flatten the block that has them, so the help text of the pair is written once.
//!
//! What generic clap structs cannot do, and how the blocks work around it:
//!
//! - A generic field with `#[arg(value_enum)]` needs `T: ValueEnum + Clone + Send + Sync + 'static`.
//! - The name of a positional comes from its field, so a generic [`OneEnum`] would show `<VALUE>`
//!   where the help always said `<OPERATION_DIRECTION>`. [`ValueName`] gives the name of each type.
//! - A doc comment on a generic field is the help of every instantiation of the struct, so the
//!   arguments whose help depends on the type have a block of their own ([`WorkspaceString`]).
//! - Flattening two instantiations of one generic block into a single command panics when the
//!   command is built (the argument names clash). No command has two enums; a test builds every
//!   command to be sure.

use clap::Args;
use clap::ValueEnum;
use komorebi_layouts::Axis;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::OperationDirection;
use komorebi_layouts::Sizing;
use komorebi_protocol::animation::prefix::AnimationPrefix;
use komorebi_protocol::pathext::replace_env_in_path;
use komorebi_protocol::types::ApplicationIdentifier;
use komorebi_protocol::types::HidingBehaviour;
use komorebi_protocol::types::MoveBehaviour;
use komorebi_protocol::types::OperationBehaviour;
use komorebi_protocol::types::StateQuery;
use komorebi_protocol::types::WindowKind;
use std::num::NonZeroUsize;
use std::path::PathBuf;

/// `enable` or `disable`, the way most of the subcommands that turn something on or off say it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum BooleanState {
    Enable,
    Disable,
}

impl From<BooleanState> for bool {
    fn from(b: BooleanState) -> Self {
        match b {
            BooleanState::Enable => true,
            BooleanState::Disable => false,
        }
    }
}

/// The name a positional of type `Self` is shown with in the help: `<OPERATION_DIRECTION>`.
pub trait ValueName {
    const NAME: &'static str;
}

macro_rules! value_names {
    ($($ty:ty => $name:literal),+ $(,)?) => {
        $(impl ValueName for $ty {
            const NAME: &'static str = $name;
        })+
    };
}

value_names! {
    OperationDirection => "OPERATION_DIRECTION",
    CycleDirection => "CYCLE_DIRECTION",
    Axis => "AXIS",
    DefaultLayout => "DEFAULT_LAYOUT",
    BooleanState => "BOOLEAN_STATE",
    StateQuery => "STATE_QUERY",
    MoveBehaviour => "MOVE_BEHAVIOUR",
    OperationBehaviour => "OPERATION_BEHAVIOUR",
    HidingBehaviour => "HIDING_BEHAVIOUR",
}

// One positional argument that is a value of an enum.
#[derive(Args, Clone, Debug)]
pub struct OneEnum<T: ValueEnum + ValueName + Clone + Send + Sync + 'static> {
    #[arg(value_enum, value_name = T::NAME)]
    pub value: T,
}

// A target given by its index.
#[derive(Args, Clone, Debug)]
pub struct Target {
    /// Target index (zero-indexed)
    pub target: usize,
}

// A workspace given by its name.
#[derive(Args, Clone, Debug)]
pub struct NamedTarget {
    /// Target workspace name
    pub workspace: String,
}

// A workspace given by the index of its monitor and its own index on it.
#[derive(Args, Clone, Debug)]
pub struct WorkspaceTarget {
    /// Monitor index (zero-indexed)
    pub monitor: usize,

    /// Workspace index on the specified monitor (zero-indexed)
    pub workspace: usize,
}

// A workspace by index and a value of an enum.
#[derive(Args, Clone, Debug)]
pub struct WorkspaceEnum<T: ValueEnum + Clone + Send + Sync + 'static> {
    #[command(flatten)]
    pub target: WorkspaceTarget,

    #[clap(value_enum)]
    pub value: T,
}

// A workspace by index and a name for it.
#[derive(Args, Clone, Debug)]
pub struct WorkspaceString {
    #[command(flatten)]
    pub target: WorkspaceTarget,

    /// Name of the workspace as a String
    pub value: String,
}

// A workspace by name and a value of an enum.
#[derive(Args, Clone, Debug)]
pub struct NamedWorkspaceEnum<T: ValueEnum + Clone + Send + Sync + 'static> {
    #[command(flatten)]
    pub target: NamedTarget,

    #[clap(value_enum)]
    pub value: T,
}

// A size in pixels.
#[derive(Args, Clone, Debug)]
pub struct Size {
    /// Pixels size to set as an integer
    pub size: i32,
}

// A padding for a workspace by index.
#[derive(Args, Clone, Debug)]
pub struct WorkspacePadding {
    #[command(flatten)]
    pub target: WorkspaceTarget,

    /// Pixels to pad with as an integer
    pub size: i32,
}

// A padding for a workspace by name.
#[derive(Args, Clone, Debug)]
pub struct NamedPadding {
    #[command(flatten)]
    pub target: NamedTarget,

    /// Pixels to pad with as an integer
    pub size: i32,
}

// A change of a padding by an amount of pixels.
#[derive(Args, Clone, Debug)]
pub struct PaddingAdjustment {
    #[clap(value_enum)]
    pub sizing: Sizing,
    /// Pixels to adjust by as an integer
    pub adjustment: i32,
}

// An application, by what identifies its windows and the identifier.
#[derive(Args, Clone, Debug)]
pub struct ApplicationTarget {
    #[clap(value_enum)]
    pub identifier: ApplicationIdentifier,
    /// Identifier as a string
    pub id: String,
}

// The arguments of the subcommands that have a block of their own, and the blocks they share.

// Which socket to send the event notifications to.
#[derive(Args, Clone, Debug)]
pub struct SubscribeSocket {
    /// Name of the socket to send event notifications to
    pub socket: String,
}

// Which socket to stop sending the event notifications to.
#[derive(Args, Clone, Debug)]
pub struct UnsubscribeSocket {
    /// Name of the socket to stop sending event notifications to
    pub socket: String,
}

// Where to save the resize layout dimensions to.
#[derive(Args, Clone, Debug)]
pub struct SaveResize {
    /// File to which the resize layout dimensions should be saved
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

// Where to load the resize layout dimensions from.
#[derive(Args, Clone, Debug)]
pub struct LoadResize {
    /// File from which the resize layout dimensions should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

// Where the static configuration of a running komorebi is read from.
#[derive(Args, Clone, Debug)]
pub struct ReplaceConfiguration {
    /// Static configuration JSON file from which the configuration should be loaded
    #[clap(value_parser = replace_env_in_path)]
    pub path: PathBuf,
}

// The edge of the focused window to resize and whether to grow or shrink it.
#[derive(Args, Clone, Debug)]
pub struct Resize {
    #[clap(value_enum)]
    pub edge: OperationDirection,
    #[clap(value_enum)]
    pub sizing: Sizing,
}

// The axis to resize along and whether to grow or shrink.
#[derive(Args, Clone, Debug)]
pub struct ResizeAxis {
    #[clap(value_enum)]
    pub axis: Axis,
    #[clap(value_enum)]
    pub sizing: Sizing,
}

// The pixels the resize commands change a window by.
#[derive(Args, Clone, Debug)]
pub struct ResizeDelta {
    /// The delta of pixels by which to increase or decrease window dimensions when resizing
    pub pixels: i32,
}

// A workspace of another monitor.
#[derive(Args, Clone, Debug)]
pub struct MonitorWorkspace {
    /// Target monitor index (zero-indexed)
    pub target_monitor: usize,
    /// Workspace index on the target monitor (zero-indexed)
    pub target_workspace: usize,
}

// The four sides of a work area offset.
#[derive(Args, Clone, Debug)]
pub struct WorkAreaOffset {
    /// Size of the left work area offset (set right to left * 2 to maintain right padding)
    pub left: i32,
    /// Size of the top work area offset (set bottom to the same value to maintain bottom padding)
    pub top: i32,
    /// Size of the right work area offset
    pub right: i32,
    /// Size of the bottom work area offset
    pub bottom: i32,
}

// The work area offset of a monitor.
#[derive(Args, Clone, Debug)]
pub struct MonitorWorkAreaOffset {
    /// Monitor index (zero-indexed)
    pub monitor: usize,
    #[command(flatten)]
    pub offset: WorkAreaOffset,
}

// The work area offset of a workspace.
#[derive(Args, Clone, Debug)]
pub struct WorkspaceWorkAreaOffset {
    /// Monitor index (zero-indexed)
    pub monitor: usize,
    /// Workspace index (zero-indexed)
    pub workspace: usize,
    #[command(flatten)]
    pub offset: WorkAreaOffset,
}

// A monitor to give an index to.
#[derive(Args, Clone, Debug)]
pub struct DisplayIndexPreference {
    /// Preferred monitor index (zero-indexed)
    pub index_preference: usize,
    /// Display name as identified in komorebic state
    pub display: String,
}

// How many workspaces a monitor has at least.
#[derive(Args, Clone, Debug)]
pub struct EnsureWorkspaces {
    /// Monitor index (zero-indexed)
    pub monitor: usize,
    /// Number of desired workspaces
    pub workspace_count: usize,
}

// The names of the workspaces a monitor has.
#[derive(Args, Clone, Debug)]
pub struct EnsureNamedWorkspaces {
    /// Monitor index (zero-indexed)
    pub monitor: usize,
    /// Names of desired workspaces
    pub names: Vec<String>,
}

// A layout a workspace changes to at a number of containers.
#[derive(Args, Clone, Debug)]
pub struct WorkspaceLayoutRule {
    #[command(flatten)]
    pub target: WorkspaceTarget,

    /// The number of window containers on-screen required to trigger this layout rule
    pub at_container_count: usize,

    #[clap(value_enum)]
    pub layout: DefaultLayout,
}

// A layout a named workspace changes to at a number of containers.
#[derive(Args, Clone, Debug)]
pub struct NamedWorkspaceLayoutRule {
    #[command(flatten)]
    pub target: NamedTarget,

    /// The number of window containers on-screen required to trigger this layout rule
    pub at_container_count: usize,

    #[clap(value_enum)]
    pub layout: DefaultLayout,
}

// A named workspace whose rules are cleared.
#[derive(Args, Clone, Debug)]
pub struct ClearNamedWorkspaceRules {
    /// Name of a workspace
    pub workspace: String,
}

// An application and the workspace its windows go to.
#[derive(Args, Clone, Debug)]
pub struct WorkspaceRule {
    #[command(flatten)]
    pub application: ApplicationTarget,
    #[command(flatten)]
    pub target: WorkspaceTarget,
}

// An application and the named workspace its windows go to.
#[derive(Args, Clone, Debug)]
pub struct NamedWorkspaceRule {
    #[command(flatten)]
    pub application: ApplicationTarget,
    /// Name of a workspace
    pub workspace: String,
}

// The width of the border.
#[derive(Args, Clone, Debug)]
pub struct BorderWidth {
    /// Desired width of the window border
    pub width: i32,
}

// The offset of the border.
#[derive(Args, Clone, Debug)]
pub struct BorderOffset {
    /// Desired offset of the window border
    pub offset: i32,
}

// The frame rate of the animations.
#[derive(Args, Clone, Debug)]
pub struct AnimationFps {
    /// Desired animation frames per second
    pub fps: u64,
}

#[derive(Args, Clone, Debug)]
pub struct Animation {
    #[clap(value_enum)]
    pub boolean_state: BooleanState,
    /// Animation type to apply the state to. If not specified, sets global state
    #[clap(value_enum, short, long)]
    pub animation_type: Option<AnimationPrefix>,
}

#[derive(Args, Clone, Debug)]
pub struct AnimationDuration {
    /// Desired animation durations in ms
    pub duration: u64,
    /// Animation type to apply the duration to. If not specified, sets global duration
    #[clap(value_enum, short, long)]
    pub animation_type: Option<AnimationPrefix>,
}

#[derive(Args, Clone, Debug)]
pub struct AnimationStyle {
    /// Desired ease function for animation
    #[clap(value_enum, short, long, default_value = "linear")]
    pub style: komorebi_protocol::animation::AnimationStyle,
    /// Animation type to apply the style to. If not specified, sets global style
    #[clap(value_enum, short, long)]
    pub animation_type: Option<AnimationPrefix>,
}

#[derive(Args, Clone, Debug)]
pub struct BorderColour {
    #[clap(value_enum, short, long, default_value = "single")]
    pub window_kind: WindowKind,
    /// Red
    pub r: u32,
    /// Green
    pub g: u32,
    /// Blue
    pub b: u32,
}

#[derive(Args, Clone, Debug)]
pub struct MonocleFocusBehaviour {
    /// Desired monocle focus behaviour
    #[clap(value_enum)]
    pub behaviour: komorebi_protocol::types::MonocleFocusBehaviour,
}

#[derive(Args, Clone, Debug)]
pub struct ScrollingLayoutColumns {
    /// Desired number of visible columns
    pub count: NonZeroUsize,
}

#[derive(Args, Clone, Debug)]
pub struct EagerFocus {
    /// Case-sensitive exe identifier
    pub exe: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Focus {
        #[command(flatten)]
        direction: OneEnum<OperationDirection>,
    }

    #[derive(Parser)]
    struct Layout {
        #[command(flatten)]
        layout: WorkspaceEnum<DefaultLayout>,
    }

    #[test]
    fn a_positional_enum_is_shown_with_the_name_of_its_type() {
        let help = <Focus as clap::CommandFactory>::command()
            .render_help()
            .to_string();
        assert!(
            help.contains("Usage: komorebic-core <OPERATION_DIRECTION>"),
            "{help}"
        );
    }

    #[test]
    fn a_flattened_pair_comes_before_the_value() {
        let help = <Layout as clap::CommandFactory>::command()
            .render_help()
            .to_string();
        assert!(
            help.contains("Usage: komorebic-core <MONITOR> <WORKSPACE> <VALUE>"),
            "{help}"
        );
        let layout = Layout::try_parse_from(["layout", "0", "1", "bsp"])
            .unwrap()
            .layout;
        assert_eq!((layout.target.monitor, layout.target.workspace), (0, 1));
        assert!(matches!(layout.value, DefaultLayout::BSP));
    }

    #[test]
    fn a_boolean_state_is_a_bool() {
        assert!(bool::from(BooleanState::Enable));
        assert!(!bool::from(BooleanState::Disable));
    }
}
