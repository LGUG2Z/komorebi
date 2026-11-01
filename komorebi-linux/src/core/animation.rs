//! The easing curves an animation can have (`komorebic animation-style`).

pub use komorebi_protocol::animation::AnimationStyle;

/// The Qt easing curve of an animation style, as KWin's animation API takes it.
///
/// A trait because `AnimationStyle` is to live in a crate shared with the other platforms, and an
/// inherent impl cannot be written outside the crate that defines the type.
pub trait QtCurve {
    /// Returns the name of the Qt easing curve of this style, as KWin's animation API takes it
    /// (`QEasingCurve.<name>`).
    ///
    /// The CSS-style easings are Qt's, without the "Ease". A custom cubic Bézier is not one of
    /// them, and is an ease out, the kind of curve a window moving to its place wants.
    ///
    /// ```rust
    /// use komorebi::core::animation::AnimationStyle;
    /// use komorebi::core::animation::QtCurve;
    ///
    /// assert_eq!(AnimationStyle::Linear.qt_curve(), "Linear");
    /// assert_eq!(AnimationStyle::EaseInOutSine.qt_curve(), "InOutSine");
    /// assert_eq!(
    ///     AnimationStyle::CubicBezier(0.3, 0.0, 0.2, 1.0).qt_curve(),
    ///     "OutCubic"
    /// );
    /// ```
    fn qt_curve(&self) -> &'static str;
}

impl QtCurve for AnimationStyle {
    fn qt_curve(&self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseInSine => "InSine",
            Self::EaseOutSine => "OutSine",
            Self::EaseInOutSine => "InOutSine",
            Self::EaseInQuad => "InQuad",
            Self::EaseOutQuad => "OutQuad",
            Self::EaseInOutQuad => "InOutQuad",
            Self::EaseInCubic => "InCubic",
            Self::EaseOutCubic => "OutCubic",
            Self::EaseInOutCubic => "InOutCubic",
            Self::EaseInQuart => "InQuart",
            Self::EaseOutQuart => "OutQuart",
            Self::EaseInOutQuart => "InOutQuart",
            Self::EaseInQuint => "InQuint",
            Self::EaseOutQuint => "OutQuint",
            Self::EaseInOutQuint => "InOutQuint",
            Self::EaseInExpo => "InExpo",
            Self::EaseOutExpo => "OutExpo",
            Self::EaseInOutExpo => "InOutExpo",
            Self::EaseInCirc => "InCirc",
            Self::EaseOutCirc => "OutCirc",
            Self::EaseInOutCirc => "InOutCirc",
            Self::EaseInBack => "InBack",
            Self::EaseOutBack => "OutBack",
            Self::EaseInOutBack => "InOutBack",
            Self::EaseInElastic => "InElastic",
            Self::EaseOutElastic => "OutElastic",
            Self::EaseInOutElastic => "InOutElastic",
            Self::EaseInBounce => "InBounce",
            Self::EaseOutBounce => "OutBounce",
            Self::EaseInOutBounce => "InOutBounce",
            Self::CubicBezier(..) => "OutCubic",
        }
    }
}
