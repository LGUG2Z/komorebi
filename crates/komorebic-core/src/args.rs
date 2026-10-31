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
use komorebi_protocol::types::ApplicationIdentifier;
use komorebi_protocol::types::HidingBehaviour;
use komorebi_protocol::types::MoveBehaviour;
use komorebi_protocol::types::OperationBehaviour;
use komorebi_protocol::types::StateQuery;

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
