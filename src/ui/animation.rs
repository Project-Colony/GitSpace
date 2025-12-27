#![allow(dead_code)]
//! Shared animation tokens and effect presets for GitSpace.
//!
//! ## Usage patterns
//! - Use [`MotionSettings::timing`] with an [`AnimationIntent`] instead of ad-hoc durations.
//! - Prefer the presets in [`AnimationEffects`] to keep fades, slides, and shadows consistent.
//! - Respect reduced motion: when enabled, timings resolve to `0ms` so UI changes are instant.
//! - Keep new animations aligned with the intent map (hover, press, focus, open/close, load).
//!
//! ```
//! use crate::ui::animation::{AnimationIntent, AnimationEffects, MotionSettings};
//!
//! let motion = MotionSettings::new(false);
//! let timing = motion.timing(AnimationIntent::Hover);
//! let fade = AnimationEffects::fade_in();
//! # let _ = (timing, fade);
//! ```

use std::time::Duration;

use crate::config::Preferences;

/// High-level intent buckets for animation decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationIntent {
    Hover,
    Press,
    Focus,
    OpenClose,
    Load,
}

/// Common easing curves used by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EasingCurve {
    Standard,
    Accelerate,
    Decelerate,
    Emphasized,
    Linear,
}

impl EasingCurve {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Accelerate => "accelerate",
            Self::Decelerate => "decelerate",
            Self::Emphasized => "emphasized",
            Self::Linear => "linear",
        }
    }

    /// Returns cubic bezier control points for the curve.
    pub const fn control_points(self) -> (f32, f32, f32, f32) {
        match self {
            Self::Standard => (0.2, 0.0, 0.0, 1.0),
            Self::Accelerate => (0.3, 0.0, 0.8, 0.15),
            Self::Decelerate => (0.0, 0.0, 0.2, 1.0),
            Self::Emphasized => (0.2, 0.0, 0.0, 1.2),
            Self::Linear => (0.0, 0.0, 1.0, 1.0),
        }
    }
}

/// Timing tokens that combine duration and easing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationTiming {
    pub duration: Duration,
    pub easing: EasingCurve,
}

impl AnimationTiming {
    pub const fn reduced(self) -> Self {
        Self {
            duration: durations::INSTANT,
            easing: self.easing,
        }
    }
}

/// Standard duration tokens.
pub mod durations {
    use std::time::Duration;

    pub const INSTANT: Duration = Duration::from_millis(0);
    pub const QUICK: Duration = Duration::from_millis(90);
    pub const SHORT: Duration = Duration::from_millis(140);
    pub const MEDIUM: Duration = Duration::from_millis(220);
    pub const LONG: Duration = Duration::from_millis(320);
}

/// Mapping of intents to timing tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationTokens;

impl AnimationTokens {
    pub const fn timing(intent: AnimationIntent) -> AnimationTiming {
        match intent {
            AnimationIntent::Hover => AnimationTiming {
                duration: durations::QUICK,
                easing: EasingCurve::Standard,
            },
            AnimationIntent::Press => AnimationTiming {
                duration: durations::QUICK,
                easing: EasingCurve::Accelerate,
            },
            AnimationIntent::Focus => AnimationTiming {
                duration: durations::SHORT,
                easing: EasingCurve::Decelerate,
            },
            AnimationIntent::OpenClose => AnimationTiming {
                duration: durations::MEDIUM,
                easing: EasingCurve::Emphasized,
            },
            AnimationIntent::Load => AnimationTiming {
                duration: durations::LONG,
                easing: EasingCurve::Standard,
            },
        }
    }
}

/// Global animation settings derived from user preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionSettings {
    reduced_motion: bool,
}

impl MotionSettings {
    pub const fn new(reduced_motion: bool) -> Self {
        Self { reduced_motion }
    }

    pub fn from_preferences(preferences: &Preferences) -> Self {
        Self::new(preferences.reduced_motion())
    }

    pub const fn reduced_motion(self) -> bool {
        self.reduced_motion
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
    }

    pub const fn timing(self, intent: AnimationIntent) -> AnimationTiming {
        let timing = AnimationTokens::timing(intent);
        if self.reduced_motion {
            timing.reduced()
        } else {
            timing
        }
    }
}

/// Opacity transition preset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FadeEffect {
    pub from_opacity: f32,
    pub to_opacity: f32,
}

/// Positional offset transition preset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideEffect {
    pub from_offset: [f32; 2],
    pub to_offset: [f32; 2],
}

/// Scale transition preset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleEffect {
    pub from_scale: f32,
    pub to_scale: f32,
}

/// Blur effect preset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurEffect {
    pub radius: f32,
}

/// Glow effect preset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowEffect {
    pub intensity: f32,
    pub radius: f32,
}

/// Shadow effect preset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowEffect {
    pub offset: [f32; 2],
    pub blur: f32,
    pub opacity: f32,
}

/// Reusable effect presets aligned with GitSpace visuals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationEffects;

impl AnimationEffects {
    pub const fn fade_in() -> FadeEffect {
        FadeEffect {
            from_opacity: 0.0,
            to_opacity: 1.0,
        }
    }

    pub const fn fade_out() -> FadeEffect {
        FadeEffect {
            from_opacity: 1.0,
            to_opacity: 0.0,
        }
    }

    pub const fn slide_up(distance: f32) -> SlideEffect {
        SlideEffect {
            from_offset: [0.0, distance],
            to_offset: [0.0, 0.0],
        }
    }

    pub const fn slide_down(distance: f32) -> SlideEffect {
        SlideEffect {
            from_offset: [0.0, -distance],
            to_offset: [0.0, 0.0],
        }
    }

    pub const fn scale_in() -> ScaleEffect {
        ScaleEffect {
            from_scale: 0.96,
            to_scale: 1.0,
        }
    }

    pub const fn scale_out() -> ScaleEffect {
        ScaleEffect {
            from_scale: 1.0,
            to_scale: 0.96,
        }
    }

    pub const fn soft_blur() -> BlurEffect {
        BlurEffect { radius: 6.0 }
    }

    pub const fn subtle_glow() -> GlowEffect {
        GlowEffect {
            intensity: 0.18,
            radius: 10.0,
        }
    }

    pub const fn soft_shadow() -> ShadowEffect {
        ShadowEffect {
            offset: [0.0, 6.0],
            blur: 16.0,
            opacity: 0.25,
        }
    }
}
