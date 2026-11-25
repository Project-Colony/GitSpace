//! The tab button of the tab bar.

use iced::widget::{button, text};
use iced::{Color, Element};

use crate::ui::theme::{with_alpha, Theme};

/// Crée un bouton tab pour la barre d'onglets.
pub fn tab_button<'a, M: Clone + 'a>(
    theme: &Theme,
    label: &'a str,
    is_active: bool,
    on_press: M,
) -> Element<'a, M> {
    let text_color = if is_active {
        theme.palette.text_primary
    } else {
        theme.palette.text_secondary
    };

    let accent = theme.palette.accent;

    let content = text(label).size(theme.typography.body).color(text_color);

    button(content)
        .on_press(on_press)
        .padding([6, 16])
        .style(move |_theme: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered if !is_active => with_alpha(accent, 0.1),
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    width: if is_active { 2.0 } else { 0.0 },
                    color: if is_active {
                        accent
                    } else {
                        Color::TRANSPARENT
                    },
                },
                text_color,
                ..Default::default()
            }
        })
        .into()
}
