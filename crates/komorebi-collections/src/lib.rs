//! The collections that every komorebi keeps its monitors, workspaces, containers and windows in.

pub mod lockable_sequence;
pub mod ring;

pub use lockable_sequence::Lockable;

// Re-exported so that `impl_ring_elements!` can name `pastey::paste!` from any crate that
// calls it, whether or not that crate depends on `pastey` itself.
#[doc(hidden)]
pub use pastey;
