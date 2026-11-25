//! Toasts: short success messages and error reports with actions.
//!
//! Success toasts use the kit's `toast`; error toasts add a body and action buttons,
//! which the kit's widget does not have.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use colony_ui::theme::Palette;
use colony_ui::widgets::icons;
use colony_ui::{fonts, Typography};
use iced::widget::{button as iced_button, column, container, row, text, Space};
use iced::{Alignment, Border, Element, Length};

use super::button;
use crate::i18n::t;

/// At most this many toasts are shown; the oldest goes first.
const MAX_TOASTS: usize = 5;

/// The end of a toast's life, during which it fades out. Longer than the app's 1 s expiry
/// tick, so a tick always lands in it and the app switches to per-frame ticks.
const FADE: Duration = Duration::from_millis(1500);

const WIDTH: f32 = 340.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
}

impl ToastKind {
    fn lifetime(self) -> Duration {
        match self {
            ToastKind::Success => Duration::from_secs(8),
            ToastKind::Error => Duration::from_secs(20),
        }
    }
}

/// A button on an error toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastAction {
    Retry,
    CopyLogPath(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Toast {
    /// Set by [`Toasts::push`]; stable while other toasts come and go.
    pub id: u64,
    pub kind: ToastKind,
    pub title: String,
    pub body: String,
    pub actions: Vec<ToastAction>,
    pub created: Instant,
}

impl Toast {
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(ToastKind::Success, message.into(), String::new())
    }

    pub fn error(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(ToastKind::Error, title.into(), body.into())
    }

    fn new(kind: ToastKind, title: String, body: String) -> Self {
        Self {
            id: 0,
            kind,
            title,
            body,
            actions: Vec::new(),
            created: Instant::now(),
        }
    }

    pub fn with_action(mut self, action: ToastAction) -> Self {
        self.actions.push(action);
        self
    }

    fn expires(&self) -> Instant {
        self.created + self.kind.lifetime()
    }

    /// 1.0 until the last moments of the toast's life, then down to 0; always 1.0 without effects.
    fn opacity(&self, now: Instant) -> f32 {
        if !colony_ui::effects_enabled() {
            return 1.0;
        }
        let left = self.expires().saturating_duration_since(now);
        (left.as_secs_f32() / FADE.as_secs_f32()).min(1.0)
    }
}

#[derive(Debug, Default)]
pub struct Toasts {
    queue: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    /// Shows a toast and returns its id.
    pub fn push(&mut self, mut toast: Toast) -> u64 {
        toast.id = self.next_id;
        self.next_id += 1;
        self.queue.push(toast);
        if self.queue.len() > MAX_TOASTS {
            self.queue.remove(0);
        }
        self.next_id - 1
    }

    pub fn dismiss(&mut self, id: u64) {
        self.queue.retain(|toast| toast.id != id);
    }

    /// Removes the toasts whose time is up at `now`.
    pub fn expire(&mut self, now: Instant) {
        self.queue.retain(|toast| toast.expires() > now);
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// True while a toast is fading, so the caller can redraw every frame.
    pub fn is_fading(&self, now: Instant) -> bool {
        self.queue.iter().any(|toast| toast.opacity(now) < 1.0)
    }

    /// The toasts stacked in a fixed-width column, newest last.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        on_dismiss: impl Fn(u64) -> M,
        on_action: impl Fn(u64, ToastAction) -> M,
    ) -> Element<'a, M> {
        if self.queue.is_empty() {
            return Space::new().into();
        }
        let typo = Typography::current();
        let now = Instant::now();
        let toasts = self.queue.iter().map(|toast| match toast.kind {
            ToastKind::Success => {
                colony_ui::widgets::toast(&typo, &toast.title, on_dismiss(toast.id))
            }
            ToastKind::Error => error_toast(&typo, toast, now, &on_dismiss, &on_action),
        });
        column(toasts).spacing(8).width(WIDTH).into()
    }
}

fn error_toast<'a, M: Clone + 'a>(
    typo: &Typography,
    toast: &'a Toast,
    now: Instant,
    on_dismiss: &impl Fn(u64) -> M,
    on_action: &impl Fn(u64, ToastAction) -> M,
) -> Element<'a, M> {
    let alpha = toast.opacity(now);
    let close = iced_button(
        text(icons::CLOSE)
            .size(typo.sz(12))
            .font(fonts::glyph_font())
            .color(Palette::TEXT_DIMMER().scale_alpha(alpha)),
    )
    .on_press(on_dismiss(toast.id))
    .padding(2)
    .style(|_theme, _status| iced_button::Style::default());

    let mut content = column![row![
        text(toast.title.as_str())
            .size(typo.sz(13))
            .font(typo.bold)
            .color(Palette::ERROR().scale_alpha(alpha)),
        Space::new().width(Length::Fill),
        close,
    ]
    .align_y(Alignment::Center),]
    .spacing(4);
    if !toast.body.is_empty() {
        content = content.push(
            text(toast.body.as_str())
                .size(typo.sz(12))
                .font(typo.regular)
                .color(Palette::TEXT_SECONDARY().scale_alpha(alpha)),
        );
    }
    if !toast.actions.is_empty() {
        let actions = toast.actions.iter().map(|action| {
            let label = match action {
                ToastAction::Retry => t("retry"),
                ToastAction::CopyLogPath(_) => t("copy_log_path"),
            };
            button::secondary(label, Some(on_action(toast.id, action.clone())))
        });
        content = content.push(row(actions).spacing(6));
    }

    container(content)
        .padding([10, 14])
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Palette::ERROR_BG().scale_alpha(alpha).into()),
            border: Border::default()
                .rounded(8)
                .width(1)
                .color(Palette::ERROR().scale_alpha(alpha * 0.5)),
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_at(toasts: &mut Toasts, toast: Toast, created: Instant) -> u64 {
        toasts.push(Toast { created, ..toast })
    }

    #[test]
    fn toasts_expire_by_kind() {
        let start = Instant::now();
        let mut toasts = Toasts::default();
        push_at(&mut toasts, Toast::success("Pushed"), start);
        push_at(&mut toasts, Toast::error("Push failed", "rejected"), start);

        toasts.expire(start + Duration::from_secs(7));
        assert_eq!(toasts.queue.len(), 2);

        toasts.expire(start + Duration::from_secs(8));
        assert_eq!(toasts.queue.len(), 1);
        assert_eq!(toasts.queue[0].kind, ToastKind::Error);

        toasts.expire(start + Duration::from_secs(19));
        assert_eq!(toasts.queue.len(), 1);

        toasts.expire(start + Duration::from_secs(20));
        assert!(toasts.is_empty());
    }

    #[test]
    fn dismiss_removes_only_that_toast() {
        let mut toasts = Toasts::default();
        let first = toasts.push(Toast::error("First", ""));
        let second = toasts.push(Toast::error("Second", ""));
        let third = toasts.push(Toast::success("Third"));
        assert_ne!(first, second);

        toasts.dismiss(second);
        let left: Vec<u64> = toasts.queue.iter().map(|toast| toast.id).collect();
        assert_eq!(left, [first, third]);
    }

    #[test]
    fn the_oldest_toast_goes_past_the_limit() {
        let mut toasts = Toasts::default();
        for i in 0..MAX_TOASTS + 2 {
            toasts.push(Toast::error(format!("Toast {i}"), ""));
        }
        assert_eq!(toasts.queue.len(), MAX_TOASTS);
        assert_eq!(toasts.queue[0].title, "Toast 2");
    }

    #[test]
    fn toasts_fade_only_with_effects() {
        let start = Instant::now();
        let toast = Toast {
            created: start,
            ..Toast::success("Fetched")
        };
        let near_end = start + ToastKind::Success.lifetime() - FADE / 2;
        assert_eq!(toast.opacity(start), 1.0);

        colony_ui::set_effects(true);
        assert!(toast.opacity(near_end) < 1.0);
        colony_ui::set_effects(false);
        assert_eq!(toast.opacity(near_end), 1.0);
        colony_ui::set_effects(true);
    }
}
