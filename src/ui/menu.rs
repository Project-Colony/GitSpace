//! Composants de menu avec style pour GitSpace Iced.
//!
//! Fournit des fonctions utilitaires pour créer des éléments de menu
//! avec le style Catppuccin et les animations hover/sélection.

use iced::widget::{button, container, row, text, Space};
use iced::{Alignment, Color, Element, Length};

use crate::ui::theme::{Theme, with_alpha};

/// Crée un élément de menu stylisé.
pub fn menu_item<'a, M: Clone + 'a>(
    theme: &Theme,
    label: &'a str,
    selected: bool,
    on_press: M,
) -> Element<'a, M> {
    let text_color = if selected {
        theme.palette.accent
    } else {
        theme.palette.text_primary
    };

    let bg_color = if selected {
        with_alpha(theme.palette.surface_highlight, 0.3)
    } else {
        Color::TRANSPARENT
    };

    let icon_color = if selected {
        with_alpha(theme.palette.accent, 0.9)
    } else {
        with_alpha(theme.palette.accent, 0.35)
    };

    let content = row![
        // Indicateur circulaire
        container(Space::new(7, 7))
            .style(move |_: &iced::Theme| container::Style {
                background: Some(icon_color.into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        Space::with_width(10),
        text(label)
            .size(theme.typography.body)
            .color(text_color),
    ]
    .align_y(Alignment::Center)
    .padding([4, 8]);

    button(content)
        .on_press(on_press)
        .padding([4, 8])
        .width(Length::Fill)
        .style(move |_theme: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered => with_alpha(bg_color, 0.4),
                button::Status::Pressed => with_alpha(bg_color, 0.5),
                _ => bg_color,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                text_color,
                ..Default::default()
            }
        })
        .into()
}

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

    let content = text(label)
        .size(theme.typography.body)
        .color(text_color);

    button(content)
        .on_press(on_press)
        .padding([6, 16])
        .style(move |_theme: &iced::Theme, status| {
            let bg = match status {
                button::Status::Hovered if !is_active => {
                    with_alpha(accent, 0.1)
                }
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(bg.into()),
                border: iced::Border {
                    radius: 4.0.into(),
                    width: if is_active { 2.0 } else { 0.0 },
                    color: if is_active { accent } else { Color::TRANSPARENT },
                },
                text_color,
                ..Default::default()
            }
        })
        .into()
}
