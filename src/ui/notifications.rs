use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationAction {
    RetryClone,
    CopyLogPath(PathBuf),
    OpenRelease(String),
    EnableTelemetry,
    DeclineTelemetry,
}

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
}

impl Notification {
    pub fn error<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Error)
    }

    pub fn success<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Success)
    }

    pub fn with_log_path(mut self, path: PathBuf) -> Self {
        self.log_path = Some(path);
        self
    }

    pub fn with_action(mut self, action: NotificationAction) -> Self {
        self.actions.push(action);
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
            duration: Duration::from_secs(12),
        }
    }
}

#[derive(Default)]
pub struct NotificationCenter {
    queue: Vec<Notification>,
}

impl NotificationCenter {
    pub fn push(&mut self, notification: Notification) {
        self.queue.push(notification);
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Vec<NotificationAction> {
        let now = Instant::now();
        self.queue
            .retain(|n| now.duration_since(n.created_at) < n.duration);

        let mut actions = Vec::new();
        for (idx, notification) in self.queue.iter_mut().enumerate() {
            let anchor = egui::Align2::RIGHT_TOP;
            let offset = egui::vec2(-12.0, 12.0 + idx as f32 * 120.0);
            egui::Area::new(format!("toast-{}", idx).into())
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
                            };
                            let text_color = Color32::WHITE;
                            let frame = egui::Frame::default()
                                .fill(fill)
                                .rounding(egui::Rounding::same(8.0))
                                .outer_margin(egui::Margin::same(4.0))
                                .inner_margin(egui::Margin::symmetric(12.0, 10.0));

                            frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.heading(
                                        egui::RichText::new(&notification.title).color(text_color),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Dismiss").clicked() {
                                                notification.duration = Duration::from_secs(0);
                                            }
                                        },
                                    );
                                });

                                ui.label(
                                    egui::RichText::new(&notification.message)
                                        .color(text_color)
                                        .small(),
                                );

                                if let Some(detail) = &notification.detail {
                                    ui.label(
                                        egui::RichText::new(detail)
                                            .color(text_color)
                                            .italics()
                                            .small(),
                                    );
                                }

                                ui.horizontal_wrapped(|ui| {
                                    for action in &notification.actions {
                                        match action {
                                            NotificationAction::RetryClone => {
                                                if ui.button("Retry").clicked() {
                                                    actions.push(action.clone());
                                                }
                                            }
                                            NotificationAction::CopyLogPath(path) => {
                                                if ui.button("Copy log path").clicked() {
                                                    actions.push(NotificationAction::CopyLogPath(
                                                        path.clone(),
                                                    ));
                                                }
                                            }
                                            NotificationAction::OpenRelease(url) => {
                                                if ui.button("Open release").clicked() {
                                                    actions.push(NotificationAction::OpenRelease(
                                                        url.clone(),
                                                    ));
                                                }
                                            }
                                            NotificationAction::EnableTelemetry => {
                                                if ui.button("Enable analytics").clicked() {
                                                    actions
                                                        .push(NotificationAction::EnableTelemetry);
                                                }
                                            }
                                            NotificationAction::DeclineTelemetry => {
                                                if ui.button("No thanks").clicked() {
                                                    actions
                                                        .push(NotificationAction::DeclineTelemetry);
                                                }
                                            }
                                        }
                                    }
                                    if let Some(path) = &notification.log_path {
                                        let target = format!("file://{}", path.display());
                                        ui.hyperlink_to("Open logs directory", target);
                                    }
                                });
                            });
                        },
                    );
                });
        }

        actions
    }
}
