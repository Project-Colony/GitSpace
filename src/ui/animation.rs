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

use eframe::egui::{self, Id};
use serde::Deserialize;
use serde_json::json;

use crate::config::Preferences;
use crate::dotnet::{DotnetClient, LibraryCallRequest};

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

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "standard" => Some(Self::Standard),
            "accelerate" => Some(Self::Accelerate),
            "decelerate" => Some(Self::Decelerate),
            "emphasized" => Some(Self::Emphasized),
            "linear" => Some(Self::Linear),
            _ => None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationTimingSet {
    pub hover: AnimationTiming,
    pub press: AnimationTiming,
    pub focus: AnimationTiming,
    pub open_close: AnimationTiming,
    pub load: AnimationTiming,
}

impl AnimationTimingSet {
    pub const fn default_tokens() -> Self {
        Self {
            hover: AnimationTokens::timing(AnimationIntent::Hover),
            press: AnimationTokens::timing(AnimationIntent::Press),
            focus: AnimationTokens::timing(AnimationIntent::Focus),
            open_close: AnimationTokens::timing(AnimationIntent::OpenClose),
            load: AnimationTokens::timing(AnimationIntent::Load),
        }
    }

    pub const fn timing(self, intent: AnimationIntent) -> AnimationTiming {
        match intent {
            AnimationIntent::Hover => self.hover,
            AnimationIntent::Press => self.press,
            AnimationIntent::Focus => self.focus,
            AnimationIntent::OpenClose => self.open_close,
            AnimationIntent::Load => self.load,
        }
    }
}

/// Global animation settings derived from user preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSettings {
    reduced_motion: bool,
    profile: AnimationProfile,
}

impl MotionSettings {
    pub const fn new(reduced_motion: bool) -> Self {
        Self {
            reduced_motion,
            profile: AnimationProfile::default_profile(),
        }
    }

    pub const fn with_profile(reduced_motion: bool, profile: AnimationProfile) -> Self {
        Self {
            reduced_motion,
            profile,
        }
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
        let timing = self.profile.timings.timing(intent);
        if self.reduced_motion {
            timing.reduced()
        } else {
            timing
        }
    }

    pub const fn effects(self) -> AnimationEffectSet {
        self.profile.effects
    }

    pub const fn slide_distance(self) -> f32 {
        self.profile.slide_distance
    }

    pub const fn slide_up(self) -> SlideEffect {
        SlideEffect {
            from_offset: [0.0, self.profile.slide_distance],
            to_offset: [0.0, 0.0],
        }
    }

    pub const fn slide_down(self) -> SlideEffect {
        SlideEffect {
            from_offset: [0.0, -self.profile.slide_distance],
            to_offset: [0.0, 0.0],
        }
    }
}

const MOTION_SETTINGS_KEY: &str = "motion_settings";

pub fn store_motion_settings(ctx: &egui::Context, preferences: &Preferences) {
    let profile = load_dotnet_animation_profile().unwrap_or_else(AnimationProfile::default_profile);
    let motion = MotionSettings::with_profile(preferences.reduced_motion(), profile);
    ctx.data_mut(|data| {
        data.insert_persisted(Id::new(MOTION_SETTINGS_KEY), motion);
    });
}

pub fn motion_settings(ctx: &egui::Context) -> MotionSettings {
    ctx.data_mut(|data| {
        data.get_persisted(Id::new(MOTION_SETTINGS_KEY))
            .unwrap_or_else(|| MotionSettings::new(false))
    })
}

/// Opacity transition preset.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct FadeEffect {
    pub from_opacity: f32,
    pub to_opacity: f32,
}

/// Positional offset transition preset.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SlideEffect {
    pub from_offset: [f32; 2],
    pub to_offset: [f32; 2],
}

/// Scale transition preset.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ScaleEffect {
    pub from_scale: f32,
    pub to_scale: f32,
}

/// Blur effect preset.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct BlurEffect {
    pub radius: f32,
}

/// Glow effect preset.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct GlowEffect {
    pub intensity: f32,
    pub radius: f32,
}

/// Shadow effect preset.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ShadowEffect {
    pub offset: [f32; 2],
    pub blur: f32,
    pub opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationEffectSet {
    pub fade_in: FadeEffect,
    pub fade_out: FadeEffect,
    pub scale_in: ScaleEffect,
    pub scale_out: ScaleEffect,
    pub soft_blur: BlurEffect,
    pub subtle_glow: GlowEffect,
    pub soft_shadow: ShadowEffect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationProfile {
    pub timings: AnimationTimingSet,
    pub effects: AnimationEffectSet,
    pub slide_distance: f32,
}

impl AnimationProfile {
    pub const fn default_profile() -> Self {
        Self {
            timings: AnimationTimingSet::default_tokens(),
            effects: AnimationEffectSet {
                fade_in: AnimationEffects::fade_in(),
                fade_out: AnimationEffects::fade_out(),
                scale_in: AnimationEffects::scale_in(),
                scale_out: AnimationEffects::scale_out(),
                soft_blur: AnimationEffects::soft_blur(),
                subtle_glow: AnimationEffects::subtle_glow(),
                soft_shadow: AnimationEffects::soft_shadow(),
            },
            slide_distance: 8.0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnimationTimingPayload {
    duration_ms: u64,
    easing: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnimationTimingSetPayload {
    hover: AnimationTimingPayload,
    press: AnimationTimingPayload,
    focus: AnimationTimingPayload,
    open_close: AnimationTimingPayload,
    load: AnimationTimingPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnimationEffectSetPayload {
    fade_in: FadeEffect,
    fade_out: FadeEffect,
    scale_in: ScaleEffect,
    scale_out: ScaleEffect,
    soft_blur: BlurEffect,
    subtle_glow: GlowEffect,
    soft_shadow: ShadowEffect,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnimationProfilePayload {
    timings: AnimationTimingSetPayload,
    effects: AnimationEffectSetPayload,
    slide_distance: f32,
}

impl AnimationProfilePayload {
    fn into_profile(self) -> Option<AnimationProfile> {
        Some(AnimationProfile {
            timings: AnimationTimingSet {
                hover: timing_from_payload(self.timings.hover)?,
                press: timing_from_payload(self.timings.press)?,
                focus: timing_from_payload(self.timings.focus)?,
                open_close: timing_from_payload(self.timings.open_close)?,
                load: timing_from_payload(self.timings.load)?,
            },
            effects: AnimationEffectSet {
                fade_in: self.effects.fade_in,
                fade_out: self.effects.fade_out,
                scale_in: self.effects.scale_in,
                scale_out: self.effects.scale_out,
                soft_blur: self.effects.soft_blur,
                subtle_glow: self.effects.subtle_glow,
                soft_shadow: self.effects.soft_shadow,
            },
            slide_distance: self.slide_distance,
        })
    }
}

fn timing_from_payload(payload: AnimationTimingPayload) -> Option<AnimationTiming> {
    let easing = EasingCurve::from_label(payload.easing.as_str())?;
    Some(AnimationTiming {
        duration: Duration::from_millis(payload.duration_ms),
        easing,
    })
}

fn load_dotnet_animation_profile() -> Option<AnimationProfile> {
    let client = DotnetClient::helper();
    let response = client
        .library_call(LibraryCallRequest {
            name: "ui.animation_profile".to_string(),
            payload: json!({}),
        })
        .ok()?;
    let payload: AnimationProfilePayload = serde_json::from_value(response.payload).ok()?;
    payload.into_profile()
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
