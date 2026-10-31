//! Which platform handles which `SocketMessage`.

use crate::message::SocketMessage;

/// A platform a komorebi daemon runs on
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Kde,
}

impl Platform {
    /// The platform this crate is compiled for. Linux is KDE: it is the only Linux daemon.
    pub const CURRENT: Self = if cfg!(target_os = "windows") {
        Self::Windows
    } else if cfg!(target_os = "macos") {
        Self::MacOs
    } else {
        Self::Kde
    };

    const fn bit(self) -> u8 {
        match self {
            Self::Windows => 1,
            Self::MacOs => 2,
            Self::Kde => 4,
        }
    }
}

/// A set of platforms
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platforms(u8);

impl Platforms {
    pub const ALL: Self = Self(7);
    pub const WINDOWS: Self = Self(1);
    pub const MACOS: Self = Self(2);
    pub const KDE: Self = Self(4);
    pub const WINDOWS_MACOS: Self = Self(1 | 2);
    pub const WINDOWS_KDE: Self = Self(1 | 4);

    #[must_use]
    pub const fn contains(self, platform: Platform) -> bool {
        self.0 & platform.bit() != 0
    }
}

impl SocketMessage {
    /// The platforms whose daemon handles this message.
    ///
    /// One exhaustive `match`: a new variant does not compile until it is placed here.
    #[must_use]
    pub const fn platforms(&self) -> Platforms {
        match self {
            Self::AddSubscriberSocket(..)
            | Self::AddSubscriberSocketWithOptions(..)
            | Self::AdjustContainerPadding(..)
            | Self::AdjustWorkspacePadding(..)
            | Self::Animation(..)
            | Self::AnimationDuration(..)
            | Self::AnimationFps(..)
            | Self::AnimationStyle(..)
            | Self::ApplicationSpecificConfigurationSchema
            | Self::Border(..)
            | Self::BorderColour(..)
            | Self::BorderOffset(..)
            | Self::BorderWidth(..)
            | Self::CancelPreselect
            | Self::CenterFocus
            | Self::ChangeLayout(..)
            | Self::ClearAllWorkspaceRules
            | Self::ClearNamedWorkspaceLayoutRules(..)
            | Self::ClearNamedWorkspaceRules(..)
            | Self::ClearSessionFloatRules
            | Self::ClearWorkspaceLayoutRules(..)
            | Self::ClearWorkspaceRules(..)
            | Self::Close
            | Self::CloseWorkspace
            | Self::Consume(..)
            | Self::ContainerPadding(..)
            | Self::CrossMonitorMoveBehaviour(..)
            | Self::CycleFocusEmptyWorkspace(..)
            | Self::CycleFocusMonitor(..)
            | Self::CycleFocusWindow(..)
            | Self::CycleFocusWorkspace(..)
            | Self::CycleLayout(..)
            | Self::CycleMoveContainerToMonitor(..)
            | Self::CycleMoveContainerToWorkspace(..)
            | Self::CycleMoveWindow(..)
            | Self::CycleMoveWorkspaceToMonitor(..)
            | Self::CycleSendContainerToMonitor(..)
            | Self::CycleSendContainerToWorkspace(..)
            | Self::CycleStack(..)
            | Self::CycleStackIndex(..)
            | Self::DebugWindow(..)
            | Self::DisplayIndexPreference(..)
            | Self::EagerFocus(..)
            | Self::EnforceWorkspaceRules
            | Self::EnsureNamedWorkspaces(..)
            | Self::EnsureWorkspaces(..)
            | Self::Expand
            | Self::Expel
            | Self::FlipLayout(..)
            | Self::FocusLastWorkspace
            | Self::FocusMonitorAtCursor
            | Self::FocusMonitorNumber(..)
            | Self::FocusMonitorWorkspaceNumber(..)
            | Self::FocusNamedWorkspace(..)
            | Self::FocusStackWindow(..)
            | Self::FocusWindow(..)
            | Self::FocusWorkspaceNumber(..)
            | Self::FocusWorkspaceNumbers(..)
            | Self::FocusedWorkspaceContainerPadding(..)
            | Self::FocusedWorkspacePadding(..)
            | Self::GenerateStaticConfig
            | Self::GlobalState
            | Self::IgnoreRule(..)
            | Self::InitialNamedWorkspaceRule(..)
            | Self::InitialWorkspaceRule(..)
            | Self::LayoutRatios(..)
            | Self::Load(..)
            | Self::LockMonitorWorkspaceContainer(..)
            | Self::ManageFocusedWindow
            | Self::ManageRule(..)
            | Self::Minimize
            | Self::MonitorInformation
            | Self::MonitorWorkAreaOffset(..)
            | Self::MonocleFocusBehaviour(..)
            | Self::MouseFollowsFocus(..)
            | Self::MoveContainerToLastWorkspace
            | Self::MoveContainerToMonitorNumber(..)
            | Self::MoveContainerToMonitorWorkspaceNumber(..)
            | Self::MoveContainerToNamedWorkspace(..)
            | Self::MoveContainerToWorkspaceNumber(..)
            | Self::MoveWindow(..)
            | Self::MoveWorkspaceToMonitorNumber(..)
            | Self::NamedWorkspaceContainerPadding(..)
            | Self::NamedWorkspaceLayout(..)
            | Self::NamedWorkspaceLayoutRule(..)
            | Self::NamedWorkspacePadding(..)
            | Self::NamedWorkspaceRule(..)
            | Self::NamedWorkspaceTiling(..)
            | Self::NewWorkspace
            | Self::NotificationSchema
            | Self::Pan(..)
            | Self::PreselectDirection(..)
            | Self::Promote
            | Self::PromoteFocus
            | Self::PromoteSwap
            | Self::PromoteWindow(..)
            | Self::Query(..)
            | Self::QuickLoad
            | Self::QuickSave
            | Self::ReloadStaticConfiguration(..)
            | Self::RemoveSubscriberSocket(..)
            | Self::ReplaceConfiguration(..)
            | Self::ResizeDelta(..)
            | Self::ResizeWindowAxis(..)
            | Self::ResizeWindowEdge(..)
            | Self::Retile
            | Self::RetileWithResizeDimensions
            | Self::Save(..)
            | Self::ScrollingLayoutColumns(..)
            | Self::ScrollingLayoutFullWidth
            | Self::SendContainerToLastWorkspace
            | Self::SendContainerToMonitorNumber(..)
            | Self::SendContainerToMonitorWorkspaceNumber(..)
            | Self::SendContainerToNamedWorkspace(..)
            | Self::SendContainerToWorkspaceNumber(..)
            | Self::SessionFloatRule
            | Self::SessionFloatRules
            | Self::SocketSchema
            | Self::StackAll
            | Self::StackWindow(..)
            | Self::State
            | Self::StaticConfigSchema
            | Self::Stop
            | Self::StopIgnoreRestore
            | Self::SwapWorkspacesToMonitorNumber(..)
            | Self::Theme(..)
            | Self::ToggleColumnStack
            | Self::ToggleCrossMonitorMoveBehaviour
            | Self::ToggleFloat
            | Self::ToggleFloatOverride
            | Self::ToggleLock
            | Self::ToggleMonocle
            | Self::ToggleMonocleFocusBehaviour
            | Self::ToggleMouseFollowsFocus
            | Self::TogglePause
            | Self::ToggleTiling
            | Self::ToggleWindowBasedWorkAreaOffset
            | Self::ToggleWindowContainerBehaviour
            | Self::ToggleWorkspaceFloatOverride
            | Self::ToggleWorkspaceLayer
            | Self::ToggleWorkspaceWindowContainerBehaviour
            | Self::UnlockMonitorWorkspaceContainer(..)
            | Self::UnmanageFocusedWindow
            | Self::UnmanagedWindowOperationBehaviour(..)
            | Self::UnstackAll
            | Self::UnstackWindow
            | Self::VisibleWindows
            | Self::WorkAreaOffset(..)
            | Self::WorkspaceLayout(..)
            | Self::WorkspaceLayoutRule(..)
            | Self::WorkspaceName(..)
            | Self::WorkspacePadding(..)
            | Self::WorkspaceRule(..)
            | Self::WorkspaceTiling(..)
            | Self::WorkspaceWorkAreaOffset(..) => Platforms::ALL,
            Self::AddSubscriberPipe(..)
            | Self::AltFocusHack(..)
            | Self::ApplyState(..)
            | Self::BorderImplementation(..)
            | Self::BorderStyle(..)
            | Self::ChangeLayoutCustom(..)
            | Self::CompleteConfiguration
            | Self::ForceFocus
            | Self::IdentifyBorderOverflowApplication(..)
            | Self::IdentifyLayeredApplication(..)
            | Self::IdentifyObjectNameChangeApplication(..)
            | Self::IdentifyTrayApplication(..)
            | Self::InvisibleBorders(..)
            | Self::MonitorIndexPreference(..)
            | Self::NamedWorkspaceLayoutCustom(..)
            | Self::NamedWorkspaceLayoutCustomRule(..)
            | Self::ReloadConfiguration
            | Self::RemoveSubscriberPipe(..)
            | Self::RemoveTitleBar(..)
            | Self::StackbarBackgroundColour(..)
            | Self::StackbarFocusedTextColour(..)
            | Self::StackbarFontFamily(..)
            | Self::StackbarFontSize(..)
            | Self::StackbarHeight(..)
            | Self::StackbarLabel(..)
            | Self::StackbarMode(..)
            | Self::StackbarTabWidth(..)
            | Self::StackbarUnfocusedTextColour(..)
            | Self::ToggleTitleBars
            | Self::WatchConfiguration(..)
            | Self::WindowHidingBehaviour(..)
            | Self::WorkspaceLayoutCustom(..)
            | Self::WorkspaceLayoutCustomRule(..) => Platforms::WINDOWS,
            Self::FocusFollowsMouse(..)
            | Self::ToggleFocusFollowsMouse(..)
            | Self::ToggleMaximize
            | Self::ToggleTransparency
            | Self::Transparency(..)
            | Self::TransparencyAlpha(..) => Platforms::WINDOWS_KDE,
            Self::PanGesture(..) => Platforms::WINDOWS_MACOS,
            Self::HotkeysState | Self::ReloadHotkeys => Platforms::KDE,
        }
    }
}

impl SocketMessage {
    /// What a daemon answers to a message of a platform it does not run on
    #[must_use]
    pub fn not_supported_reply(&self) -> String {
        format!("{self} is not supported on this platform")
    }
}
