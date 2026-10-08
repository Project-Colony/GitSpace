//! Système de notifications toast avec auto-dismiss et limites.

#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length};

use crate::ui::theme::Theme;

/// Nombre maximum de notifications affichées simultanément.
const MAX_NOTIFICATIONS: usize = 5;

/// Durée par défaut avant auto-dismiss (secondes).
const DEFAULT_DURATION_SECS: u64 = 12;

/// Durée pour les erreurs (plus longue pour laisser le temps de lire).
const ERROR_DURATION_SECS: u64 = 20;

/// Type de notification pour le style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Success,
    Error,
    Info,
}

/// Actions déclenchables depuis les boutons de notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationAction {
    RetryClone,
    CopyLogPath(PathBuf),
    OpenRelease(String),
}

/// Une notification toast avec actions optionnelles.
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
    /// Crée une notification d'erreur avec durée plus longue.
    pub fn error<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Error)
            .with_duration(Duration::from_secs(ERROR_DURATION_SECS))
    }

    /// Crée une notification de succès.
    pub fn success<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Success)
    }

    /// Crée une notification d'information.
    pub fn info<T: Into<String>, D: Into<String>>(title: T, detail: D) -> Self {
        Self::new(title, detail, NotificationKind::Info)
    }

    /// Attache un chemin de log à la notification.
    pub fn with_log_path(mut self, path: PathBuf) -> Self {
        self.log_path = Some(path);
        self
    }

    /// Ajoute un bouton d'action à la notification.
    pub fn with_action(mut self, action: NotificationAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Définit une durée d'affichage personnalisée.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Définit le texte de détail additionnel.
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

    /// Retourne true si la notification doit être retirée.
    fn is_expired(&self) -> bool {
        self.dismissed || Instant::now().duration_since(self.created_at) >= self.duration
    }

    /// Marque la notification comme dismissée.
    fn dismiss(&mut self) {
        self.dismissed = true;
    }

    /// Retourne la progression (0.0 à 1.0) dans la durée de vie de la notification.
    pub fn progress(&self) -> f32 {
        let elapsed = Instant::now().duration_since(self.created_at);
        (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// Messages du centre de notifications.
#[derive(Debug, Clone)]
pub enum Message {
    Dismiss(usize),
    ActionClicked(usize, NotificationAction),
    Tick,
}

/// Conteneur pour gérer et afficher les notifications.
#[derive(Default)]
pub struct NotificationCenter {
    queue: Vec<Notification>,
}

impl NotificationCenter {
    /// Crée un nouveau centre de notifications.
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Ajoute une notification à la file.
    pub fn push(&mut self, notification: Notification) {
        self.queue.push(notification);
        while self.queue.len() > MAX_NOTIFICATIONS {
            self.queue.remove(0);
        }
    }

    /// Retourne le nombre de notifications actives.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Retourne true s'il n'y a aucune notification.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Dismiss toutes les notifications.
    pub fn dismiss_all(&mut self) {
        self.queue.clear();
    }

    /// Retourne true si des notifications sont visibles (pour la subscription de tick).
    pub fn has_visible(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Gère un message de notification. Retourne les actions déclenchées.
    pub fn update(&mut self, message: Message) -> Vec<NotificationAction> {
        match message {
            Message::Dismiss(idx) => {
                if let Some(notification) = self.queue.get_mut(idx) {
                    notification.dismiss();
                }
                Vec::new()
            }
            Message::ActionClicked(idx, action) => {
                if let Some(notification) = self.queue.get_mut(idx) {
                    notification.dismiss();
                }
                vec![action]
            }
            Message::Tick => {
                self.queue.retain(|n| !n.is_expired());
                Vec::new()
            }
        }
    }

    /// Construit la vue des notifications en overlay.
    pub fn view(&self, theme: &Theme) -> Element<'_, Message> {
        if self.queue.is_empty() {
            return Space::new(0, 0).into();
        }

        let mut notifications = column![].spacing(8).width(Length::Fixed(340.0));

        for (idx, notification) in self.queue.iter().enumerate() {
            let fill_color = match notification.kind {
                NotificationKind::Success => Color::from_rgb8(26, 102, 64),
                NotificationKind::Error => Color::from_rgb8(125, 32, 32),
                NotificationKind::Info => Color::from_rgb8(32, 64, 125),
            };

            let mut content = column![].spacing(4);

            // Titre + bouton dismiss
            let title_row = row![
                text(&notification.title)
                    .size(theme.typography.body)
                    .color(Color::WHITE),
                Space::with_width(Length::Fill),
                button(text("x").size(12).color(Color::WHITE))
                    .on_press(Message::Dismiss(idx))
                    .padding(2),
            ]
            .align_y(Alignment::Center);
            content = content.push(title_row);

            // Message
            content = content.push(
                text(&notification.message)
                    .size(theme.typography.label)
                    .color(Color::from_rgba8(255, 255, 255, 0.85)),
            );

            // Détail optionnel
            if let Some(detail) = &notification.detail {
                content = content.push(
                    text(detail)
                        .size(theme.typography.label)
                        .color(Color::from_rgba8(255, 255, 255, 0.7)),
                );
            }

            // Boutons d'action
            if !notification.actions.is_empty() {
                let mut action_row = row![].spacing(6);
                for action in &notification.actions {
                    let label = match action {
                        NotificationAction::RetryClone => "Réessayer",
                        NotificationAction::CopyLogPath(_) => "Copier le chemin log",
                        NotificationAction::OpenRelease(_) => "Ouvrir la release",
                    };
                    action_row = action_row.push(
                        button(text(label).size(12).color(Color::WHITE))
                            .on_press(Message::ActionClicked(idx, action.clone()))
                            .padding([2, 6]),
                    );
                }
                content = content.push(action_row);
            }

            let toast = container(content)
                .padding(12)
                .style(move |_theme: &iced::Theme| container::Style {
                    background: Some(fill_color.into()),
                    border: iced::Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .width(Length::Fill);

            notifications = notifications.push(toast);
        }

        container(notifications)
            .width(Length::Fixed(340.0))
            .into()
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
