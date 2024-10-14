use crate::animation::animation_manager::AnimationManager;
use crate::core::animation::AnimationStyle;

use lazy_static::lazy_static;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use parking_lot::Mutex;

pub mod animation;
pub use animation::Animation;
pub mod animation_manager;
pub mod lerp;
pub mod prefix;
pub mod render_dispatcher;
pub use render_dispatcher::RenderDispatcher;
pub mod style;

lazy_static! {
    pub static ref ANIMATION_STYLE: Arc<Mutex<AnimationStyle>> =
        Arc::new(Mutex::new(AnimationStyle::Linear));
    pub static ref ANIMATION_MANAGER: Arc<Mutex<AnimationManager>> =
        Arc::new(Mutex::new(AnimationManager::new()));
}

pub static ANIMATION_ENABLED: AtomicBool = AtomicBool::new(false);
pub static ANIMATION_DURATION: AtomicU64 = AtomicU64::new(250);
pub static ANIMATION_FPS: AtomicU64 = AtomicU64::new(60);
