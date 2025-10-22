use crate::animation::animation_manager::AnimationManager;
use crate::core::animation::AnimationStyle;

use lazy_static::lazy_static;
use prefix::AnimationPrefix;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;

use parking_lot::Mutex;

pub use engine::AnimationEngine;
pub mod animation_manager;
pub mod engine;
pub mod lerp;
pub mod prefix;
pub mod render_dispatcher;
pub use render_dispatcher::RenderDispatcher;
pub mod style;

use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum PerAnimationPrefixConfig<T> {
    Prefix(HashMap<AnimationPrefix, T>),
    Global(T),
}

pub const DEFAULT_ANIMATION_ENABLED: bool = false;
pub const DEFAULT_ANIMATION_STYLE: AnimationStyle = AnimationStyle::Linear;
pub const DEFAULT_ANIMATION_DURATION: u64 = 250;
pub const DEFAULT_ANIMATION_FPS: u64 = 60;

lazy_static! {
    pub static ref ANIMATION_MANAGER: Arc<Mutex<AnimationManager>> =
        Arc::new(Mutex::new(AnimationManager::new()));
    pub static ref ANIMATION_STYLE_GLOBAL: Arc<Mutex<AnimationStyle>> =
        Arc::new(Mutex::new(DEFAULT_ANIMATION_STYLE));
    pub static ref ANIMATION_ENABLED_GLOBAL: Arc<AtomicBool> =
        Arc::new(AtomicBool::new(DEFAULT_ANIMATION_ENABLED));
    pub static ref ANIMATION_DURATION_GLOBAL: Arc<AtomicU64> =
        Arc::new(AtomicU64::new(DEFAULT_ANIMATION_DURATION));
    pub static ref ANIMATION_STYLE_PER_ANIMATION: Arc<Mutex<HashMap<AnimationPrefix, AnimationStyle>>> =
        Arc::new(Mutex::new(HashMap::new()));
    pub static ref ANIMATION_ENABLED_PER_ANIMATION: Arc<Mutex<HashMap<AnimationPrefix, bool>>> =
        Arc::new(Mutex::new(HashMap::new()));
    pub static ref ANIMATION_DURATION_PER_ANIMATION: Arc<Mutex<HashMap<AnimationPrefix, u64>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub static ANIMATION_FPS: AtomicU64 = AtomicU64::new(DEFAULT_ANIMATION_FPS);
