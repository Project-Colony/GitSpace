//! Tokens d'animation et presets d'effets pour GitSpace.
//!
//! ## Patterns d'utilisation
//! - Utiliser [`MotionSettings::timing`] avec un [`AnimationIntent`] au lieu de durées ad-hoc.
//! - Préférer les presets dans [`AnimationEffects`] pour garder les fades, slides et shadows cohérents.
//! - Respecter le mode réduit : quand activé, les timings deviennent `0ms` (transitions instantanées).
//! - Garder les nouvelles animations alignées avec la carte des intents (hover, press, focus, open/close, load).
//!
//! En mode Iced, les animations sont pilotées par des subscriptions et un `AnimationState`
//! qui track la progression de chaque animation active.

#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;

use crate::config::{MotionIntensity, Preferences};

/// Buckets d'intent de haut niveau pour les décisions d'animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationIntent {
    Hover,
    Press,
    Focus,
    OpenClose,
    Load,
}

/// Courbes d'easing communes utilisées par l'UI.
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

    /// Retourne les points de contrôle cubic bezier pour la courbe.
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

/// Tokens de timing qui combinent durée et easing.
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

/// Tokens de durée standard.
pub mod durations {
    use std::time::Duration;

    pub const INSTANT: Duration = Duration::from_millis(0);
    pub const QUICK: Duration = Duration::from_millis(90);
    pub const SHORT: Duration = Duration::from_millis(140);
    pub const MEDIUM: Duration = Duration::from_millis(220);
    pub const LONG: Duration = Duration::from_millis(320);
}

/// Mapping des intents vers les tokens de timing.
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

/// Paramètres d'animation globaux dérivés des préférences utilisateur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionSettings {
    reduced_motion: bool,
    intensity: MotionIntensity,
    performance_mode: bool,
    profile: AnimationProfile,
}

impl MotionSettings {
    pub const fn new(reduced_motion: bool) -> Self {
        Self {
            reduced_motion,
            intensity: MotionIntensity::Medium,
            performance_mode: false,
            profile: AnimationProfile::default_profile(),
        }
    }

    pub const fn with_profile(
        reduced_motion: bool,
        intensity: MotionIntensity,
        performance_mode: bool,
        profile: AnimationProfile,
    ) -> Self {
        Self {
            reduced_motion,
            intensity,
            performance_mode,
            profile,
        }
    }

    pub fn from_preferences(preferences: &Preferences) -> Self {
        Self::with_profile(
            preferences.reduced_motion(),
            preferences.motion_intensity(),
            preferences.performance_mode(),
            AnimationProfile::default_profile(),
        )
    }

    pub const fn reduced_motion(self) -> bool {
        self.reduced_motion
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
    }

    pub fn timing(self, intent: AnimationIntent) -> AnimationTiming {
        let timing = self.profile.timings.timing(intent);
        if self.reduced_motion {
            timing.reduced()
        } else {
            AnimationTiming {
                duration: scale_duration(timing.duration, self.duration_scale()),
                easing: timing.easing,
            }
        }
    }

    pub fn effects(self) -> AnimationEffectSet {
        scale_effects(self.profile.effects, self.effect_scale())
    }

    pub fn slide_distance(self) -> f32 {
        self.profile.slide_distance * self.effect_scale()
    }

    pub fn slide_up(self) -> SlideEffect {
        let distance = self.slide_distance();
        SlideEffect {
            from_offset: [0.0, distance],
            to_offset: [0.0, 0.0],
        }
    }

    pub fn slide_down(self) -> SlideEffect {
        let distance = self.slide_distance();
        SlideEffect {
            from_offset: [0.0, -distance],
            to_offset: [0.0, 0.0],
        }
    }

    fn duration_scale(self) -> f32 {
        let mut scale = match self.intensity {
            MotionIntensity::Low => 0.8,
            MotionIntensity::Medium => 1.0,
            MotionIntensity::High => 1.2,
        };
        if self.performance_mode {
            scale *= 0.85;
        }
        scale
    }

    fn effect_scale(self) -> f32 {
        let mut scale = match self.intensity {
            MotionIntensity::Low => 0.85,
            MotionIntensity::Medium => 1.0,
            MotionIntensity::High => 1.15,
        };
        if self.performance_mode {
            scale *= 0.7;
        }
        scale
    }
}

/// Preset de transition d'opacité.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct FadeEffect {
    pub from_opacity: f32,
    pub to_opacity: f32,
}

/// Preset de transition de position.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SlideEffect {
    pub from_offset: [f32; 2],
    pub to_offset: [f32; 2],
}

/// Preset de transition d'échelle.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ScaleEffect {
    pub from_scale: f32,
    pub to_scale: f32,
}

/// Preset d'effet de flou.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct BlurEffect {
    pub radius: f32,
}

/// Preset d'effet de glow.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct GlowEffect {
    pub intensity: f32,
    pub radius: f32,
}

/// Preset d'effet d'ombre.
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

fn scale_duration(duration: Duration, scale: f32) -> Duration {
    if scale <= 0.0 {
        return durations::INSTANT;
    }
    let scaled_ms = (duration.as_millis() as f32 * scale).round().max(0.0);
    Duration::from_millis(scaled_ms as u64)
}

fn scale_effects(effects: AnimationEffectSet, scale: f32) -> AnimationEffectSet {
    AnimationEffectSet {
        fade_in: effects.fade_in,
        fade_out: effects.fade_out,
        scale_in: scale_scale_effect(effects.scale_in, scale),
        scale_out: scale_scale_effect(effects.scale_out, scale),
        soft_blur: BlurEffect {
            radius: effects.soft_blur.radius * scale,
        },
        subtle_glow: GlowEffect {
            intensity: effects.subtle_glow.intensity * scale,
            radius: effects.subtle_glow.radius * scale,
        },
        soft_shadow: ShadowEffect {
            offset: [
                effects.soft_shadow.offset[0] * scale,
                effects.soft_shadow.offset[1] * scale,
            ],
            blur: effects.soft_shadow.blur * scale,
            opacity: (effects.soft_shadow.opacity * scale).clamp(0.0, 1.0),
        },
    }
}

fn scale_scale_effect(effect: ScaleEffect, scale: f32) -> ScaleEffect {
    ScaleEffect {
        from_scale: 1.0 - (1.0 - effect.from_scale) * scale,
        to_scale: 1.0 + (effect.to_scale - 1.0) * scale,
    }
}

/// Presets d'effets réutilisables alignés avec le design GitSpace.
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

// ─── État d'animation pour le mode retenu (Iced) ────────────────────────

/// Identifiant unique pour une animation.
pub type AnimationId = u64;

/// Animation active avec progression et timing.
struct ActiveAnimation {
    target: bool,
    progress: f32,
    timing: AnimationTiming,
}

/// Gestionnaire d'état des animations pour le mode retenu d'Iced.
///
/// Chaque widget peut enregistrer une animation via `set_target`,
/// et lire sa progression avec `progress`. Le tick global met à jour
/// toutes les animations actives.
pub struct AnimationState {
    animations: HashMap<AnimationId, ActiveAnimation>,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self {
            animations: HashMap::new(),
        }
    }
}

impl AnimationState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Définit la cible d'une animation (true = vers 1.0, false = vers 0.0).
    pub fn set_target(&mut self, id: AnimationId, target: bool, timing: AnimationTiming) {
        let anim = self.animations.entry(id).or_insert(ActiveAnimation {
            target,
            progress: if target { 0.0 } else { 1.0 },
            timing,
        });
        anim.target = target;
        anim.timing = timing;
    }

    /// Met à jour toutes les animations actives. Retourne true si au moins
    /// une animation est encore en cours (besoin de continuer le tick).
    pub fn tick(&mut self, dt: Duration) -> bool {
        let mut any_active = false;
        self.animations.retain(|_, anim| {
            let target_val = if anim.target { 1.0 } else { 0.0 };
            if (anim.progress - target_val).abs() < f32::EPSILON {
                // Animation terminée, on retire si cible est 0
                return anim.target;
            }

            let duration_secs = anim.timing.duration.as_secs_f32().max(0.001);
            let step = dt.as_secs_f32() / duration_secs;

            if anim.target {
                anim.progress = (anim.progress + step).min(1.0);
            } else {
                anim.progress = (anim.progress - step).max(0.0);
            }

            any_active = true;
            true
        });
        any_active
    }

    /// Retourne la progression actuelle d'une animation (0.0 à 1.0).
    pub fn progress(&self, id: AnimationId) -> f32 {
        self.animations
            .get(&id)
            .map(|a| a.progress)
            .unwrap_or(0.0)
    }

    /// Retourne true si au moins une animation est en cours.
    pub fn is_animating(&self) -> bool {
        self.animations.values().any(|a| {
            let target_val = if a.target { 1.0 } else { 0.0 };
            (a.progress - target_val).abs() > f32::EPSILON
        })
    }
}
