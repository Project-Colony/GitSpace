//! Toast notification system with auto-dismiss and limits.

// Public API methods are designed for future use
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32};

/// Maximum number of notifications shown at once.
const MAX_NOTIFICATIONS: usize = 5;

/// Default duration before auto-dismiss (seconds).
const DEFAULT_DURATION_SECS: u64 = 12;

/// Duration for error notifications (seconds) - longer to give time to read.
const ERROR_DURATION_SECS: u64 = 20;

/// Type of notification for styling purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Success,
    Error,
    Info,
}

/// Actions that can be triggered from notification buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationAction {
    RetryClone,
    CopyLogPath(PathBuf),
}

/// A toast notification with optional actions.
#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub kind: NotificationKind,
    pub log_path: Option<PathBuf>,
    pub actions: Vec<NotificationAction>,
    created_at: Instant,
    duration: Duration,
    dismissed: bool,
}

impl Notification {
    /// Creates an error notification with longer display duration.
    pub fn error<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Error)
            .with_duration(Duration::from_secs(ERROR_DURATION_SECS))
    }

    /// Creates a success notification.
    pub fn success<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Success)
    }

    /// Creates an info notification.
    pub fn info<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Info)
    }

    /// Attaches a log path to the notification.
    pub fn with_log_path(mut self, path: PathBuf) -> Self {
        self.log_path = Some(path);
        self
    }

    /// Adds an action button to the notification.
    pub fn with_action(mut self, action: NotificationAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Sets a custom display duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Sets additional detail text.
    pub fn with_detail<D: Into<String>>(mut self, detail: D) -> Self {
        self.detail = Some(detail.into());
        self
    }

    fn new<T: Into<String>, D: Into<String>>(title: T, detail: D, kind: NotificationKind) -> Self {
        Self {
            title: title.into(),
            message: detail.into(),
            detail: None,
            kind,
            log_path: None,
            actions: Vec::new(),
            created_at: Instant::now(),
            duration: Duration::from_secs(DEFAULT_DURATION_SECS),
            dismissed: false,
        }
    }

    /// Returns true if the notification should be removed.
    fn is_expired(&self) -> bool {
        self.dismissed || Instant::now().duration_since(self.created_at) >= self.duration
    }

    /// Marks the notification as dismissed.
    fn dismiss(&mut self) {
        self.dismissed = true;
    }

    /// Returns the progress (0.0 to 1.0) through the notification's lifetime.
    fn progress(&self) -> f32 {
        let elapsed = Instant::now().duration_since(self.created_at);
        (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// Container for managing and displaying notifications.
#[derive(Default)]
pub struct NotificationCenter {
    queue: Vec<Notification>,
}

impl NotificationCenter {
    /// Creates a new notification center.
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Adds a notification to the queue.
    ///
    /// If the queue exceeds `MAX_NOTIFICATIONS`, the oldest notifications are removed.
    pub fn push(&mut self, notification: Notification) {
        self.queue.push(notification);

        // Remove oldest notifications if over limit
        while self.queue.len() > MAX_NOTIFICATIONS {
            self.queue.remove(0);
        }
    }

    /// Returns the current number of active notifications.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns true if there are no notifications.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Dismisses all notifications.
    pub fn dismiss_all(&mut self) {
        self.queue.clear();
    }

    /// Shows all active notifications and returns any triggered actions.
    pub fn show(&mut self, ctx: &egui::Context) -> Vec<NotificationAction> {
        // Remove expired notifications
        self.queue.retain(|n| !n.is_expired());

        let mut actions = Vec::new();

        for (idx, notification) in self.queue.iter_mut().enumerate() {
            let anchor = egui::Align2::RIGHT_TOP;
            let offset = egui::vec2(-12.0, 12.0 + idx as f32 * 120.0);

            egui::Area::new(egui::Id::new(("toast", idx)))
                .anchor(anchor, offset)
                .show(ctx, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(320.0, 110.0),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

                            let fill = match notification.kind {
                                NotificationKind::Success => Color32::from_rgb(26, 102, 64),
                                NotificationKind::Error => Color32::from_rgb(125, 32, 32),
                                NotificationKind::Info => Color32::from_rgb(32, 64, 125),
                            };
                            let text_color = Color32::WHITE;

                            let frame = egui::Frame::default()
                                .fill(fill)
                                .rounding(egui::Rounding::same(8.0))
                                .outer_margin(egui::Margin::same(4.0))
                                .inner_margin(egui::Margin::symmetric(12.0, 10.0));

                            frame.show(ui, |ui| {
                                // Header with title and dismiss button
                                ui.horizontal(|ui| {
                                    ui.heading(
                                        egui::RichText::new(&notification.title).color(text_color),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("×").clicked() {
                                                notification.dismiss();
                                            }
                                        },
                                    );
                                });

                                // Message
                                ui.label(
                                    egui::RichText::new(&notification.message)
                                        .color(text_color)
                                        .small(),
                                );

                                // Detail (if present)
                                if let Some(detail) = &notification.detail {
                                    ui.label(
                                        egui::RichText::new(detail)
                                            .color(text_color)
                                            .italics()
                                            .small(),
                                    );
                                }

                                // Progress bar showing time remaining
                                let progress = notification.progress();
                                let remaining_width = ui.available_width() * (1.0 - progress);
                                let bar_height = 3.0;
                                let bar_rect = egui::Rect::from_min_size(
                                    ui.cursor().min,
                                    egui::vec2(remaining_width, bar_height),
                                );
                                ui.painter().rect_filled(
                                    bar_rect,
                                    egui::Rounding::same(1.5),
                                    Color32::from_white_alpha(80),
                                );
                                ui.add_space(bar_height + 4.0);

                                // Action buttons
                                let mut clicked_action = None;
                                ui.horizontal_wrapped(|ui| {
                                    for action in &notification.actions {
                                        let clicked = match action {
                                            NotificationAction::RetryClone => {
                                                ui.button("Retry").clicked()
                                            }
                                            NotificationAction::CopyLogPath(_) => {
                                                ui.button("Copy log path").clicked()
                                            }
                                        };
                                        if clicked {
                                            clicked_action = Some(action.clone());
                                        }
                                    }

                                    if let Some(path) = &notification.log_path {
                                        let target = format!("file://{}", path.display());
                                        ui.hyperlink_to("Open logs", target);
                                    }
                                });

                                // Handle clicked action after the loop
                                if let Some(action) = clicked_action {
                                    actions.push(action);
                                    notification.dismiss();
                                }
                            });
                        },
                    );
                });
        }

        // Request repaint if we have notifications (for progress bar animation)
        if !self.queue.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_defaults() {
        let n = Notification::success("Test", "Message");
        assert_eq!(n.kind, NotificationKind::Success);
        assert!(!n.is_expired());
    }

    #[test]
    fn error_has_longer_duration() {
        let success = Notification::success("Test", "Message");
        let error = Notification::error("Test", "Message");
        assert!(error.duration > success.duration);
    }

    #[test]
    fn notification_center_respects_limit() {
        let mut center = NotificationCenter::new();

        for i in 0..10 {
            center.push(Notification::success(format!("Test {i}"), "Message"));
        }

        assert_eq!(center.len(), MAX_NOTIFICATIONS);
    }

    #[test]
    fn dismiss_removes_notification() {
        let mut n = Notification::success("Test", "Message");
        assert!(!n.is_expired());
        n.dismiss();
        assert!(n.is_expired());
    }

    #[test]
    fn dismiss_all_clears_queue() {
        let mut center = NotificationCenter::new();
        center.push(Notification::success("Test 1", "Message"));
        center.push(Notification::success("Test 2", "Message"));

        assert_eq!(center.len(), 2);
        center.dismiss_all();
        assert!(center.is_empty());
    }
}
