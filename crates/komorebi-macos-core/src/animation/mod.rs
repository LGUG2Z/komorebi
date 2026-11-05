//! The pure part of the window animations: the easing functions, the motion of a frame, the tracks a
//! window follows, and the throttle on the frames. Nothing here reads a clock it was not given or
//! talks to the window server.

pub mod lerp;
pub mod stats;
pub mod step;
pub mod style;
pub mod throttle;
pub mod tracks;
pub mod transition;
