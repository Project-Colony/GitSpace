//! Système de notifications toast avec auto-dismiss et limites.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length};

use crate::ui::theme::Theme;

/// Nombre maximum de notifications affichées simultanément.
const MAX_NOTIFICATIONS: usize = 5;

/// Durée d'affichage d'une erreur avant auto-dismiss (secondes).
const ERROR_DURATION_SECS: u64 = 20;

/// Actions déclenchables depuis les boutons de notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationAction {
    RetryClone,
    CopyLogPath(PathBuf),
}

/// Une notification toast d'erreur avec actions optionnelles.
#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub actions: Vec<NotificationAction>,
    /// Identifiant attribué par le centre, stable quand d'autres toasts disparaissent.
    id: u64,
    created_at: Instant,
    duration: Duration,
}

impl Notification {
    /// Crée une notification d'erreur.
    pub fn error<T: Into<String>, D: Into<String>>(title: T, message: D) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            detail: None,
            actions: Vec::new(),
            id: 0,
            created_at: Instant::now(),
            duration: Duration::from_secs(ERROR_DURATION_SECS),
        }
    }

    /// Ajoute un bouton d'action à la notification.
    pub fn with_action(mut self, action: NotificationAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Retourne true si la notification doit être retirée.
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }
}

/// Messages du centre de notifications.
#[derive(Debug, Clone)]
pub enum Message {
    Dismiss(u64),
    ActionClicked(u64, NotificationAction),
    Tick,
}

/// Conteneur pour gérer et afficher les notifications.
#[derive(Default)]
pub struct NotificationCenter {
    queue: Vec<Notification>,
    next_id: u64,
}

impl NotificationCenter {
    /// Crée un nouveau centre de notifications.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute une notification à la file.
    pub fn push(&mut self, mut notification: Notification) {
        notification.id = self.next_id;
        self.next_id += 1;
        self.queue.push(notification);
        while self.queue.len() > MAX_NOTIFICATIONS {
            self.queue.remove(0);
        }
    }

    /// Retourne true s'il n'y a aucune notification.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Gère un message de notification. Retourne les actions déclenchées.
    pub fn update(&mut self, message: Message) -> Vec<NotificationAction> {
        match message {
            Message::Dismiss(id) => {
                self.queue.retain(|n| n.id != id);
                Vec::new()
            }
            Message::ActionClicked(id, action) => {
                self.queue.retain(|n| n.id != id);
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
            return Space::new().into();
        }

        let fill_color = Color::from_rgb8(125, 32, 32);
        let mut notifications = column![].spacing(8).width(Length::Fixed(340.0));

        for notification in &self.queue {
            let id = notification.id;
            let mut content = column![].spacing(4);

            // Titre + bouton dismiss
            let title_row = row![
                text(&notification.title)
                    .size(theme.typography.body)
                    .color(Color::WHITE),
                Space::new().width(Length::Fill),
                button(text("x").size(12).color(Color::WHITE))
                    .on_press(Message::Dismiss(id))
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
                    };
                    action_row = action_row.push(
                        button(text(label).size(12).color(Color::WHITE))
                            .on_press(Message::ActionClicked(id, action.clone()))
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

        container(notifications).width(Length::Fixed(340.0)).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_notification_is_not_expired() {
        assert!(!Notification::error("Test", "Message").is_expired());
    }

    #[test]
    fn notification_center_respects_limit() {
        let mut center = NotificationCenter::new();
        for i in 0..10 {
            center.push(Notification::error(format!("Test {i}"), "Message"));
        }
        assert_eq!(center.queue.len(), MAX_NOTIFICATIONS);
        assert_eq!(center.queue[0].title, "Test 5");
    }

    #[test]
    fn dismiss_removes_the_clicked_notification_at_once() {
        let mut center = NotificationCenter::new();
        center.push(Notification::error("Test 1", "Message"));
        center.push(Notification::error("Test 2", "Message"));
        let second = center.queue[1].id;
        center.update(Message::Dismiss(second));
        assert_eq!(center.queue.len(), 1);
        assert_eq!(center.queue[0].title, "Test 1");
    }
}
