//! The subcommands that are one message to the daemon.
//!
//! Every subcommand in [`Messages`] is the same in the three `komorebic`s: the same arguments, the
//! same help, one [`SocketMessage`] written to the socket (or, for a question, written and
//! answered). The conversion to the message is one exhaustive `match`, so a subcommand added here
//! cannot be left without its message.

use crate::args::*;
use clap::Subcommand;
use komorebi_layouts::Axis;
use komorebi_layouts::CycleDirection;
use komorebi_layouts::DefaultLayout;
use komorebi_layouts::OperationDirection;
use komorebi_layouts::Rect;
use komorebi_protocol::message::SocketMessage;
use komorebi_protocol::types::MoveBehaviour;
use komorebi_protocol::types::OperationBehaviour;
use komorebi_protocol::types::StateQuery;

#[derive(Subcommand, Clone, Debug)]
pub enum Messages {
    /// Show a JSON representation of the current window manager state
    State,
    /// Show a JSON representation of the current global state
    GlobalState,
    /// Show a JSON representation of visible windows
    VisibleWindows,
    /// Show information about connected monitors
    #[clap(alias = "monitor-info")]
    MonitorInformation,
    /// Query the current window manager state
    #[clap(arg_required_else_help = true)]
    Query(OneEnum<StateQuery>),
    /// Subscribe to komorebi events using a Unix Domain Socket
    #[clap(arg_required_else_help = true)]
    SubscribeSocket(SubscribeSocket),
    /// Unsubscribe from komorebi events
    #[clap(arg_required_else_help = true)]
    UnsubscribeSocket(UnsubscribeSocket),
    /// Quicksave the current resize layout dimensions
    #[clap(alias = "quick-save")]
    QuickSaveResize,
    /// Load the last quicksaved resize layout dimensions
    #[clap(alias = "quick-load")]
    QuickLoadResize,
    /// Save the current resize layout dimensions to a file
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "save")]
    SaveResize(SaveResize),
    /// Load the resize layout dimensions from a file
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "load")]
    LoadResize(LoadResize),
    /// Change focus to the window in the specified direction
    #[clap(arg_required_else_help = true)]
    Focus(OneEnum<OperationDirection>),
    /// Move the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    Move(OneEnum<OperationDirection>),
    /// Preselect the specified direction for the next window to be spawned on supported layouts
    #[clap(arg_required_else_help = true)]
    PreselectDirection(OneEnum<OperationDirection>),
    /// Cancel a workspace preselect set by the preselect-direction command, if one exists
    CancelPreselect,
    /// Minimize the focused window
    Minimize,
    /// Close the focused window
    Close,
    /// Change focus to the window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleFocus(OneEnum<CycleDirection>),
    /// Move the focused window in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMove(OneEnum<CycleDirection>),
    /// Stack the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    Stack(OneEnum<OperationDirection>),
    /// Unstack the focused window
    Unstack,
    /// Cycle the focused stack in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleStack(OneEnum<CycleDirection>),
    /// Cycle the index of the focused window in the focused stack in the specified cycle direction
    #[clap(arg_required_else_help = true)]
    CycleStackIndex(OneEnum<CycleDirection>),
    /// Focus the specified window index in the focused stack
    #[clap(arg_required_else_help = true)]
    FocusStackWindow(Target),
    /// Stack all windows on the focused workspace
    StackAll,
    /// Unstack all windows in the focused container
    UnstackAll,
    /// Resize the focused window in the specified direction
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "resize")]
    ResizeEdge(Resize),
    /// Resize the focused window or primary column along the specified axis
    #[clap(arg_required_else_help = true)]
    ResizeAxis(ResizeAxis),
    /// Move the focused window to the specified monitor
    #[clap(arg_required_else_help = true)]
    MoveToMonitor(Target),
    /// Move the focused window to the monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMoveToMonitor(OneEnum<CycleDirection>),
    /// Move the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    MoveToWorkspace(Target),
    /// Move the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    MoveToNamedWorkspace(NamedTarget),
    /// Move the focused window to the workspace in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMoveToWorkspace(OneEnum<CycleDirection>),
    /// Send the focused window to the specified monitor
    #[clap(arg_required_else_help = true)]
    SendToMonitor(Target),
    /// Send the focused window to the monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleSendToMonitor(OneEnum<CycleDirection>),
    /// Send the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    SendToWorkspace(Target),
    /// Send the focused window to the specified workspace
    #[clap(arg_required_else_help = true)]
    SendToNamedWorkspace(NamedTarget),
    /// Send the focused window to the workspace in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleSendToWorkspace(OneEnum<CycleDirection>),
    /// Send the focused window to the specified monitor workspace
    #[clap(arg_required_else_help = true)]
    SendToMonitorWorkspace(MonitorWorkspace),
    /// Move the focused window to the specified monitor workspace
    #[clap(arg_required_else_help = true)]
    MoveToMonitorWorkspace(MonitorWorkspace),
    /// Send the focused window to the last focused monitor workspace
    SendToLastWorkspace,
    /// Move the focused window to the last focused monitor workspace
    MoveToLastWorkspace,
    /// Focus the specified monitor
    #[clap(arg_required_else_help = true)]
    FocusMonitor(Target),
    /// Focus the monitor at the current cursor location
    FocusMonitorAtCursor,
    /// Focus the last focused workspace on the focused monitor
    FocusLastWorkspace,
    /// Focus the specified workspace on the focused monitor
    #[clap(arg_required_else_help = true)]
    FocusWorkspace(Target),
    /// Focus the specified workspace on all monitors
    #[clap(arg_required_else_help = true)]
    FocusWorkspaces(Target),
    /// Focus the specified workspace on the target monitor
    #[clap(arg_required_else_help = true)]
    FocusMonitorWorkspace(MonitorWorkspace),
    /// Focus the specified workspace
    #[clap(arg_required_else_help = true)]
    FocusNamedWorkspace(NamedTarget),
    /// Close the focused workspace (must be empty and unnamed)
    CloseWorkspace,
    /// Focus the monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMonitor(OneEnum<CycleDirection>),
    /// Focus the workspace in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleWorkspace(OneEnum<CycleDirection>),
    /// Focus the next empty workspace in the given cycle direction (if one exists)
    #[clap(arg_required_else_help = true)]
    CycleEmptyWorkspace(OneEnum<CycleDirection>),
    /// Move the focused workspace to the specified monitor
    #[clap(arg_required_else_help = true)]
    MoveWorkspaceToMonitor(Target),
    /// Move the focused workspace monitor in the given cycle direction
    #[clap(arg_required_else_help = true)]
    CycleMoveWorkspaceToMonitor(OneEnum<CycleDirection>),
    /// Swap focused monitor workspaces with specified monitor
    #[clap(arg_required_else_help = true)]
    SwapWorkspacesWithMonitor(Target),
    /// Create and append a new workspace on the focused monitor
    NewWorkspace,
    /// Set the resize delta (used by resize-edge and resize-axis)
    #[clap(arg_required_else_help = true)]
    ResizeDelta(ResizeDelta),
    /// Set offsets to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    GlobalWorkAreaOffset(WorkAreaOffset),
    /// Set offsets for a monitor to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    MonitorWorkAreaOffset(MonitorWorkAreaOffset),
    /// Set offsets for a workspace to exclude parts of the work area from tiling
    #[clap(arg_required_else_help = true)]
    WorkspaceWorkAreaOffset(WorkspaceWorkAreaOffset),
    /// Toggle application of the window-based work area offset for the focused workspace
    ToggleWindowBasedWorkAreaOffset,
    /// Set container padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    FocusedWorkspaceContainerPadding(Size),
    /// Set workspace padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    FocusedWorkspacePadding(Size),
    /// Adjust container padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    AdjustContainerPadding(PaddingAdjustment),
    /// Adjust workspace padding on the focused workspace
    #[clap(arg_required_else_help = true)]
    AdjustWorkspacePadding(PaddingAdjustment),
    /// Set the layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ChangeLayout(OneEnum<DefaultLayout>),
    /// Cycle between available layouts
    #[clap(arg_required_else_help = true)]
    CycleLayout(OneEnum<CycleDirection>),
    /// Make the focused column of the Scrolling layout fill the room that the columns in view leave
    /// on the screen, which does nothing in any other layout
    Expand,
    /// Flip the layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    FlipLayout(OneEnum<Axis>),
    /// Promote the focused window to the largest tile via container removal and re-insertion
    Promote,
    /// Promote the focused window to the largest tile by swapping container indices with the largest tile
    PromoteSwap,
    /// Promote the window in the specified direction
    PromoteWindow(OneEnum<OperationDirection>),
    /// Set the display index preference for a monitor identified using its display name
    #[clap(arg_required_else_help = true)]
    DisplayIndexPreference(DisplayIndexPreference),
    /// Create at least this many workspaces for the specified monitor
    #[clap(arg_required_else_help = true)]
    EnsureWorkspaces(EnsureWorkspaces),
    /// Create these many named workspaces for the specified monitor
    #[clap(arg_required_else_help = true)]
    EnsureNamedWorkspaces(EnsureNamedWorkspaces),
    /// Set the container padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    ContainerPadding(WorkspacePadding),
    /// Set the container padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceContainerPadding(NamedPadding),
    /// Set the workspace padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspacePadding(WorkspacePadding),
    /// Set the workspace padding for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspacePadding(NamedPadding),
    /// Set the layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceLayout(WorkspaceEnum<DefaultLayout>),
    /// Set the layout for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceLayout(NamedWorkspaceEnum<DefaultLayout>),
    /// Add a dynamic layout rule for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceLayoutRule(WorkspaceLayoutRule),
    /// Add a dynamic layout rule for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceLayoutRule(NamedWorkspaceLayoutRule),
    /// Clear all dynamic layout rules for the specified workspace
    #[clap(arg_required_else_help = true)]
    ClearWorkspaceLayoutRules(WorkspaceTarget),
    /// Clear all dynamic layout rules for the specified workspace
    #[clap(arg_required_else_help = true)]
    ClearNamedWorkspaceLayoutRules(NamedTarget),
    /// Enable or disable window tiling for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceTiling(WorkspaceEnum<BooleanState>),
    /// Enable or disable window tiling for the specified workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceTiling(NamedWorkspaceEnum<BooleanState>),
    /// Set the workspace name for the specified workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceName(WorkspaceString),
    /// Enable or disable float override, which makes it so every new window opens in floating mode
    ToggleFloatOverride,
    /// Enable or disable float override, which makes it so every new window opens in floating
    /// mode, for the currently focused workspace. If there was no override value set for the
    /// workspace previously it takes the opposite of the global value.
    ToggleWorkspaceFloatOverride,
    /// Toggle between the Tiling and Floating layers on the focused workspace
    ToggleWorkspaceLayer,
    /// Toggle the paused state for all window tiling
    TogglePause,
    /// Toggle window tiling on the focused workspace
    ToggleTiling,
    /// Toggle floating mode for the focused window
    ToggleFloat,
    /// Toggle monocle mode for the focused container
    ToggleMonocle,
    /// Toggle a lock for the focused container, ensuring it will not be displaced by any new windows
    ToggleLock,
    /// Force komorebi to manage the focused window
    Manage,
    /// Unmanage a window that was forcibly managed
    Unmanage,
    /// Set the behaviour when moving windows across monitor boundaries
    #[clap(arg_required_else_help = true)]
    CrossMonitorMoveBehaviour(OneEnum<MoveBehaviour>),
    /// Toggle the behaviour when moving windows across monitor boundaries
    ToggleCrossMonitorMoveBehaviour,
    /// Add a rule to float the foreground window for the rest of this session
    SessionFloatRule,
    /// Show all session float rules
    SessionFloatRules,
    /// Clear all session float rules
    ClearSessionFloatRules,
    /// Add a rule to ignore the specified application
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "float-rule")]
    IgnoreRule(ApplicationTarget),
    /// Add a rule to always manage the specified application
    #[clap(arg_required_else_help = true)]
    ManageRule(ApplicationTarget),
    /// Add a rule to associate an application with a workspace on first show
    #[clap(arg_required_else_help = true)]
    InitialWorkspaceRule(WorkspaceRule),
    /// Add a rule to associate an application with a named workspace on first show
    #[clap(arg_required_else_help = true)]
    InitialNamedWorkspaceRule(NamedWorkspaceRule),
    /// Add a rule to associate an application with a workspace
    #[clap(arg_required_else_help = true)]
    WorkspaceRule(WorkspaceRule),
    /// Add a rule to associate an application with a named workspace
    #[clap(arg_required_else_help = true)]
    NamedWorkspaceRule(NamedWorkspaceRule),
    /// Remove all application association rules for a workspace by monitor and workspace index
    #[clap(arg_required_else_help = true)]
    ClearWorkspaceRules(WorkspaceTarget),
    /// Remove all application association rules for a named workspace
    #[clap(arg_required_else_help = true)]
    ClearNamedWorkspaceRules(ClearNamedWorkspaceRules),
    /// Remove all application association rules for all workspaces
    ClearAllWorkspaceRules,
    /// Enforce all workspace rules, including initial workspace rules that have already been applied
    EnforceWorkspaceRules,
    /// Enable or disable borders
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border")]
    Border(OneEnum<BooleanState>),
    /// Set the border width
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border-width")]
    BorderWidth(BorderWidth),
    /// Set the border offset
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border-offset")]
    BorderOffset(BorderOffset),
    /// Set the frames per second for movement animations
    #[clap(arg_required_else_help = true)]
    AnimationFps(AnimationFps),
    /// Enable or disable mouse follows focus on all workspaces
    #[clap(arg_required_else_help = true)]
    MouseFollowsFocus(OneEnum<BooleanState>),
    /// Toggle mouse follows focus on all workspaces
    ToggleMouseFollowsFocus,
    /// Replace the configuration of a running instance of komorebi from a static configuration file
    #[clap(arg_required_else_help = true)]
    ReplaceConfiguration(ReplaceConfiguration),
    /// Enable or disable movement animations
    #[clap(arg_required_else_help = true)]
    Animation(Animation),
    /// Set the duration for movement animations in ms
    #[clap(arg_required_else_help = true)]
    AnimationDuration(AnimationDuration),
    /// Set the ease function for movement animations
    #[clap(arg_required_else_help = true)]
    AnimationStyle(AnimationStyle),
    /// Set the colour for a window border kind
    #[clap(arg_required_else_help = true)]
    #[clap(alias = "active-window-border-colour")]
    BorderColour(BorderColour),
    /// Set the behaviour when focusing in a direction while a monocle container is active
    #[clap(arg_required_else_help = true)]
    MonocleFocusBehaviour(MonocleFocusBehaviour),
    /// Toggle the behaviour when focusing in a direction while a monocle container is active
    ToggleMonocleFocusBehaviour,
    /// Toggle the behaviour for new windows (stacking or dynamic tiling)
    ToggleWindowContainerBehaviour,
    /// Toggle the behaviour for new windows (stacking or dynamic tiling) for currently focused
    /// workspace. If there was no behaviour set for the workspace previously it takes the opposite
    /// of the global value.
    ToggleWorkspaceWindowContainerBehaviour,
    /// Set the operation behaviour when the focused window is not managed
    #[clap(arg_required_else_help = true)]
    UnmanagedWindowOperationBehaviour(OneEnum<OperationBehaviour>),
    /// Set the number of visible columns for the Scrolling layout on the focused workspace
    #[clap(arg_required_else_help = true)]
    ScrollingLayoutColumns(ScrollingLayoutColumns),
    /// Make the focused column of the Scrolling layout as wide as the screen, or put it back to
    /// the width that it had
    ScrollingLayoutFullWidth,
    /// Take the focused window out of its column in the Scrolling layout and put it in a column
    /// of its own on the right
    Expel,
    /// Force the retiling of all managed windows
    Retile,
    /// Scroll the Scrolling layout so that the focused column is in the middle of the screen, once
    CenterFocus,
    /// Take the neighbouring window on one side into the focused column of the Scrolling layout
    #[clap(arg_required_else_help = true)]
    Consume(OneEnum<OperationDirection>),
    /// Stack the windows of the focused column of the Scrolling layout, or spread the focused
    /// stack out into a column with a window in each slot
    ToggleColumnStack,
    /// Focus the first managed window matching the given executable
    #[clap(arg_required_else_help = true)]
    EagerFocus(EagerFocus),
    /// Promote the user focus to the largest window container
    PromoteFocus,
}

impl Messages {
    /// Whether the daemon answers the message, and the answer is to be printed, rather than the
    /// message being an order.
    pub fn is_query(&self) -> bool {
        matches!(
            self,
            Self::State
                | Self::GlobalState
                | Self::VisibleWindows
                | Self::MonitorInformation
                | Self::Query(_)
                | Self::SessionFloatRules
        )
    }
}

impl From<Messages> for SocketMessage {
    fn from(messages: Messages) -> Self {
        match messages {
            Messages::State => SocketMessage::State,
            Messages::GlobalState => SocketMessage::GlobalState,
            Messages::VisibleWindows => SocketMessage::VisibleWindows,
            Messages::MonitorInformation => SocketMessage::MonitorInformation,
            Messages::Query(args) => SocketMessage::Query(args.value),
            Messages::SubscribeSocket(args) => SocketMessage::AddSubscriberSocket(args.socket),
            Messages::UnsubscribeSocket(args) => SocketMessage::RemoveSubscriberSocket(args.socket),
            Messages::QuickSaveResize => SocketMessage::QuickSave,
            Messages::QuickLoadResize => SocketMessage::QuickLoad,
            Messages::SaveResize(args) => SocketMessage::Save(args.path),
            Messages::LoadResize(args) => SocketMessage::Load(args.path),
            Messages::Focus(args) => SocketMessage::FocusWindow(args.value),
            Messages::Move(args) => SocketMessage::MoveWindow(args.value),
            Messages::PreselectDirection(args) => SocketMessage::PreselectDirection(args.value),
            Messages::CancelPreselect => SocketMessage::CancelPreselect,
            Messages::Minimize => SocketMessage::Minimize,
            Messages::Close => SocketMessage::Close,
            Messages::CycleFocus(args) => SocketMessage::CycleFocusWindow(args.value),
            Messages::CycleMove(args) => SocketMessage::CycleMoveWindow(args.value),
            Messages::Stack(args) => SocketMessage::StackWindow(args.value),
            Messages::Unstack => SocketMessage::UnstackWindow,
            Messages::CycleStack(args) => SocketMessage::CycleStack(args.value),
            Messages::CycleStackIndex(args) => SocketMessage::CycleStackIndex(args.value),
            Messages::FocusStackWindow(args) => SocketMessage::FocusStackWindow(args.target),
            Messages::StackAll => SocketMessage::StackAll,
            Messages::UnstackAll => SocketMessage::UnstackAll,
            Messages::ResizeEdge(args) => SocketMessage::ResizeWindowEdge(args.edge, args.sizing),
            Messages::ResizeAxis(args) => SocketMessage::ResizeWindowAxis(args.axis, args.sizing),
            Messages::MoveToMonitor(args) => {
                SocketMessage::MoveContainerToMonitorNumber(args.target)
            }
            Messages::CycleMoveToMonitor(args) => {
                SocketMessage::CycleMoveContainerToMonitor(args.value)
            }
            Messages::MoveToWorkspace(args) => {
                SocketMessage::MoveContainerToWorkspaceNumber(args.target)
            }
            Messages::MoveToNamedWorkspace(args) => {
                SocketMessage::MoveContainerToNamedWorkspace(args.workspace)
            }
            Messages::CycleMoveToWorkspace(args) => {
                SocketMessage::CycleMoveContainerToWorkspace(args.value)
            }
            Messages::SendToMonitor(args) => {
                SocketMessage::SendContainerToMonitorNumber(args.target)
            }
            Messages::CycleSendToMonitor(args) => {
                SocketMessage::CycleSendContainerToMonitor(args.value)
            }
            Messages::SendToWorkspace(args) => {
                SocketMessage::SendContainerToWorkspaceNumber(args.target)
            }
            Messages::SendToNamedWorkspace(args) => {
                SocketMessage::SendContainerToNamedWorkspace(args.workspace)
            }
            Messages::CycleSendToWorkspace(args) => {
                SocketMessage::CycleSendContainerToWorkspace(args.value)
            }
            Messages::SendToMonitorWorkspace(args) => {
                SocketMessage::SendContainerToMonitorWorkspaceNumber(
                    args.target_monitor,
                    args.target_workspace,
                )
            }
            Messages::MoveToMonitorWorkspace(args) => {
                SocketMessage::MoveContainerToMonitorWorkspaceNumber(
                    args.target_monitor,
                    args.target_workspace,
                )
            }
            Messages::SendToLastWorkspace => SocketMessage::SendContainerToLastWorkspace,
            Messages::MoveToLastWorkspace => SocketMessage::MoveContainerToLastWorkspace,
            Messages::FocusMonitor(args) => SocketMessage::FocusMonitorNumber(args.target),
            Messages::FocusMonitorAtCursor => SocketMessage::FocusMonitorAtCursor,
            Messages::FocusLastWorkspace => SocketMessage::FocusLastWorkspace,
            Messages::FocusWorkspace(args) => SocketMessage::FocusWorkspaceNumber(args.target),
            Messages::FocusWorkspaces(args) => SocketMessage::FocusWorkspaceNumbers(args.target),
            Messages::FocusMonitorWorkspace(args) => SocketMessage::FocusMonitorWorkspaceNumber(
                args.target_monitor,
                args.target_workspace,
            ),
            Messages::FocusNamedWorkspace(args) => {
                SocketMessage::FocusNamedWorkspace(args.workspace)
            }
            Messages::CloseWorkspace => SocketMessage::CloseWorkspace,
            Messages::CycleMonitor(args) => SocketMessage::CycleFocusMonitor(args.value),
            Messages::CycleWorkspace(args) => SocketMessage::CycleFocusWorkspace(args.value),
            Messages::CycleEmptyWorkspace(args) => {
                SocketMessage::CycleFocusEmptyWorkspace(args.value)
            }
            Messages::MoveWorkspaceToMonitor(args) => {
                SocketMessage::MoveWorkspaceToMonitorNumber(args.target)
            }
            Messages::CycleMoveWorkspaceToMonitor(args) => {
                SocketMessage::CycleMoveWorkspaceToMonitor(args.value)
            }
            Messages::SwapWorkspacesWithMonitor(args) => {
                SocketMessage::SwapWorkspacesToMonitorNumber(args.target)
            }
            Messages::NewWorkspace => SocketMessage::NewWorkspace,
            Messages::ResizeDelta(args) => SocketMessage::ResizeDelta(args.pixels),
            Messages::GlobalWorkAreaOffset(args) => SocketMessage::WorkAreaOffset(Rect {
                left: args.left,
                top: args.top,
                right: args.right,
                bottom: args.bottom,
            }),
            Messages::MonitorWorkAreaOffset(args) => SocketMessage::MonitorWorkAreaOffset(
                args.monitor,
                Rect {
                    left: args.offset.left,
                    top: args.offset.top,
                    right: args.offset.right,
                    bottom: args.offset.bottom,
                },
            ),
            Messages::WorkspaceWorkAreaOffset(args) => SocketMessage::WorkspaceWorkAreaOffset(
                args.monitor,
                args.workspace,
                Rect {
                    left: args.offset.left,
                    top: args.offset.top,
                    right: args.offset.right,
                    bottom: args.offset.bottom,
                },
            ),
            Messages::ToggleWindowBasedWorkAreaOffset => {
                SocketMessage::ToggleWindowBasedWorkAreaOffset
            }
            Messages::FocusedWorkspaceContainerPadding(args) => {
                SocketMessage::FocusedWorkspaceContainerPadding(args.size)
            }
            Messages::FocusedWorkspacePadding(args) => {
                SocketMessage::FocusedWorkspacePadding(args.size)
            }
            Messages::AdjustContainerPadding(args) => {
                SocketMessage::AdjustContainerPadding(args.sizing, args.adjustment)
            }
            Messages::AdjustWorkspacePadding(args) => {
                SocketMessage::AdjustWorkspacePadding(args.sizing, args.adjustment)
            }
            Messages::ChangeLayout(args) => SocketMessage::ChangeLayout(args.value),
            Messages::CycleLayout(args) => SocketMessage::CycleLayout(args.value),
            Messages::Expand => SocketMessage::Expand,
            Messages::FlipLayout(args) => SocketMessage::FlipLayout(args.value),
            Messages::Promote => SocketMessage::Promote,
            Messages::PromoteSwap => SocketMessage::PromoteSwap,
            Messages::PromoteWindow(args) => SocketMessage::PromoteWindow(args.value),
            Messages::DisplayIndexPreference(args) => {
                SocketMessage::DisplayIndexPreference(args.index_preference, args.display)
            }
            Messages::EnsureWorkspaces(args) => {
                SocketMessage::EnsureWorkspaces(args.monitor, args.workspace_count)
            }
            Messages::EnsureNamedWorkspaces(args) => {
                SocketMessage::EnsureNamedWorkspaces(args.monitor, args.names)
            }
            Messages::ContainerPadding(args) => SocketMessage::ContainerPadding(
                args.target.monitor,
                args.target.workspace,
                args.size,
            ),
            Messages::NamedWorkspaceContainerPadding(args) => {
                SocketMessage::NamedWorkspaceContainerPadding(args.target.workspace, args.size)
            }
            Messages::WorkspacePadding(args) => SocketMessage::WorkspacePadding(
                args.target.monitor,
                args.target.workspace,
                args.size,
            ),
            Messages::NamedWorkspacePadding(args) => {
                SocketMessage::NamedWorkspacePadding(args.target.workspace, args.size)
            }
            Messages::WorkspaceLayout(args) => SocketMessage::WorkspaceLayout(
                args.target.monitor,
                args.target.workspace,
                args.value,
            ),
            Messages::NamedWorkspaceLayout(args) => {
                SocketMessage::NamedWorkspaceLayout(args.target.workspace, args.value)
            }
            Messages::WorkspaceLayoutRule(args) => SocketMessage::WorkspaceLayoutRule(
                args.target.monitor,
                args.target.workspace,
                args.at_container_count,
                args.layout,
            ),
            Messages::NamedWorkspaceLayoutRule(args) => SocketMessage::NamedWorkspaceLayoutRule(
                args.target.workspace,
                args.at_container_count,
                args.layout,
            ),
            Messages::ClearWorkspaceLayoutRules(args) => {
                SocketMessage::ClearWorkspaceLayoutRules(args.monitor, args.workspace)
            }
            Messages::ClearNamedWorkspaceLayoutRules(args) => {
                SocketMessage::ClearNamedWorkspaceLayoutRules(args.workspace)
            }
            Messages::WorkspaceTiling(args) => SocketMessage::WorkspaceTiling(
                args.target.monitor,
                args.target.workspace,
                args.value.into(),
            ),
            Messages::NamedWorkspaceTiling(args) => {
                SocketMessage::NamedWorkspaceTiling(args.target.workspace, args.value.into())
            }
            Messages::WorkspaceName(args) => {
                SocketMessage::WorkspaceName(args.target.monitor, args.target.workspace, args.value)
            }
            Messages::ToggleFloatOverride => SocketMessage::ToggleFloatOverride,
            Messages::ToggleWorkspaceFloatOverride => SocketMessage::ToggleWorkspaceFloatOverride,
            Messages::ToggleWorkspaceLayer => SocketMessage::ToggleWorkspaceLayer,
            Messages::TogglePause => SocketMessage::TogglePause,
            Messages::ToggleTiling => SocketMessage::ToggleTiling,
            Messages::ToggleFloat => SocketMessage::ToggleFloat,
            Messages::ToggleMonocle => SocketMessage::ToggleMonocle,
            Messages::ToggleLock => SocketMessage::ToggleLock,
            Messages::Manage => SocketMessage::ManageFocusedWindow,
            Messages::Unmanage => SocketMessage::UnmanageFocusedWindow,
            Messages::CrossMonitorMoveBehaviour(args) => {
                SocketMessage::CrossMonitorMoveBehaviour(args.value)
            }
            Messages::ToggleCrossMonitorMoveBehaviour => {
                SocketMessage::ToggleCrossMonitorMoveBehaviour
            }
            Messages::SessionFloatRule => SocketMessage::SessionFloatRule,
            Messages::SessionFloatRules => SocketMessage::SessionFloatRules,
            Messages::ClearSessionFloatRules => SocketMessage::ClearSessionFloatRules,
            Messages::IgnoreRule(args) => SocketMessage::IgnoreRule(args.identifier, args.id),
            Messages::ManageRule(args) => SocketMessage::ManageRule(args.identifier, args.id),
            Messages::InitialWorkspaceRule(args) => SocketMessage::InitialWorkspaceRule(
                args.application.identifier,
                args.application.id,
                args.target.monitor,
                args.target.workspace,
            ),
            Messages::InitialNamedWorkspaceRule(args) => SocketMessage::InitialNamedWorkspaceRule(
                args.application.identifier,
                args.application.id,
                args.workspace,
            ),
            Messages::WorkspaceRule(args) => SocketMessage::WorkspaceRule(
                args.application.identifier,
                args.application.id,
                args.target.monitor,
                args.target.workspace,
            ),
            Messages::NamedWorkspaceRule(args) => SocketMessage::NamedWorkspaceRule(
                args.application.identifier,
                args.application.id,
                args.workspace,
            ),
            Messages::ClearWorkspaceRules(args) => {
                SocketMessage::ClearWorkspaceRules(args.monitor, args.workspace)
            }
            Messages::ClearNamedWorkspaceRules(args) => {
                SocketMessage::ClearNamedWorkspaceRules(args.workspace)
            }
            Messages::ClearAllWorkspaceRules => SocketMessage::ClearAllWorkspaceRules,
            Messages::EnforceWorkspaceRules => SocketMessage::EnforceWorkspaceRules,
            Messages::Border(args) => SocketMessage::Border(args.value.into()),
            Messages::BorderWidth(args) => SocketMessage::BorderWidth(args.width),
            Messages::BorderOffset(args) => SocketMessage::BorderOffset(args.offset),
            Messages::AnimationFps(args) => SocketMessage::AnimationFps(args.fps),
            Messages::MouseFollowsFocus(args) => {
                SocketMessage::MouseFollowsFocus(args.value.into())
            }
            Messages::ToggleMouseFollowsFocus => SocketMessage::ToggleMouseFollowsFocus,
            Messages::ReplaceConfiguration(args) => SocketMessage::ReplaceConfiguration(args.path),
            Messages::Animation(args) => {
                SocketMessage::Animation(args.boolean_state.into(), args.animation_type)
            }
            Messages::AnimationDuration(args) => {
                SocketMessage::AnimationDuration(args.duration, args.animation_type)
            }
            Messages::AnimationStyle(args) => {
                SocketMessage::AnimationStyle(args.style, args.animation_type)
            }
            Messages::BorderColour(args) => {
                SocketMessage::BorderColour(args.window_kind, args.r, args.g, args.b)
            }
            Messages::MonocleFocusBehaviour(args) => {
                SocketMessage::MonocleFocusBehaviour(args.behaviour)
            }
            Messages::ToggleMonocleFocusBehaviour => SocketMessage::ToggleMonocleFocusBehaviour,
            Messages::ToggleWindowContainerBehaviour => {
                SocketMessage::ToggleWindowContainerBehaviour
            }
            Messages::ToggleWorkspaceWindowContainerBehaviour => {
                SocketMessage::ToggleWorkspaceWindowContainerBehaviour
            }
            Messages::UnmanagedWindowOperationBehaviour(args) => {
                SocketMessage::UnmanagedWindowOperationBehaviour(args.value)
            }
            Messages::ScrollingLayoutColumns(args) => {
                SocketMessage::ScrollingLayoutColumns(args.count)
            }
            Messages::ScrollingLayoutFullWidth => SocketMessage::ScrollingLayoutFullWidth,
            Messages::Expel => SocketMessage::Expel,
            Messages::Retile => SocketMessage::Retile,
            Messages::CenterFocus => SocketMessage::CenterFocus,
            Messages::Consume(args) => SocketMessage::Consume(args.value),
            Messages::ToggleColumnStack => SocketMessage::ToggleColumnStack,
            Messages::EagerFocus(args) => SocketMessage::EagerFocus(args.exe),
            Messages::PromoteFocus => SocketMessage::PromoteFocus,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_printed_and_an_order_is_not() {
        assert!(Messages::State.is_query());
        assert!(!Messages::Minimize.is_query());
    }

    #[test]
    fn a_message_converts_to_the_socket_message_it_names() {
        let message: SocketMessage = Messages::ToggleMonocle.into();
        assert!(matches!(message, SocketMessage::ToggleMonocle));
        let message: SocketMessage = Messages::FocusMonitor(Target { target: 2 }).into();
        assert!(matches!(message, SocketMessage::FocusMonitorNumber(2)));
    }
}
