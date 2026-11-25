//! Push buttons in four strengths, plus a glyph button with a tooltip.
//!
//! `on_press: None` draws the button disabled.

use colony_ui::theme::{contrast_on, Palette};
use colony_ui::{fonts, Typography};
use iced::widget::{button, container, text, tooltip};
use iced::{Border, Color, Element};

#[derive(Clone, Copy)]
enum Kind {
    Primary,
    Secondary,
    Danger,
    Ghost,
}

/// The accent button: the one action a screen expects next.
pub fn primary<'a, M: Clone + 'a>(label: &str, on_press: Option<M>) -> Element<'a, M> {
    labelled(label, on_press, Kind::Primary)
}

pub fn secondary<'a, M: Clone + 'a>(label: &str, on_press: Option<M>) -> Element<'a, M> {
    labelled(label, on_press, Kind::Secondary)
}

/// For actions that lose work; they still go through a confirmation.
pub fn danger<'a, M: Clone + 'a>(label: &str, on_press: Option<M>) -> Element<'a, M> {
    labelled(label, on_press, Kind::Danger)
}

/// No background until hovered.
pub fn ghost<'a, M: Clone + 'a>(label: &str, on_press: Option<M>) -> Element<'a, M> {
    labelled(label, on_press, Kind::Ghost)
}

/// A Nerd Font glyph drawn as a ghost button, with `tip` shown on hover.
pub fn icon<'a, M: Clone + 'a>(glyph: &str, on_press: Option<M>, tip: &str) -> Element<'a, M> {
    let typo = Typography::current();
    let glyph = text(glyph.to_string())
        .size(typo.sz(14))
        .font(fonts::glyph_font());
    let body = button(glyph)
        .on_press_maybe(on_press)
        .padding([4, 6])
        .style(style(Kind::Ghost));
    tooltip(
        body,
        container(
            text(tip.to_string())
                .size(typo.sz(11))
                .font(typo.regular)
                .color(Palette::TEXT_SECONDARY()),
        )
        .padding([4, 8])
        .style(|_| container::Style {
            background: Some(Palette::BG_SIDEBAR().into()),
            border: Border::default()
                .rounded(6)
                .width(1)
                .color(Palette::BG_SELECTED()),
            ..Default::default()
        }),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn labelled<'a, M: Clone + 'a>(label: &str, on_press: Option<M>, kind: Kind) -> Element<'a, M> {
    let typo = Typography::current();
    button(text(label.to_string()).size(typo.sz(12)).font(typo.regular))
        .on_press_maybe(on_press)
        .padding([6, 12])
        .style(style(kind))
        .into()
}

fn style(kind: Kind) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, text_color) = match kind {
            Kind::Primary => {
                let accent = Palette::ACCENT();
                let bg = if hovered {
                    accent.scale_alpha(0.85)
                } else {
                    accent
                };
                (bg, contrast_on(accent))
            }
            Kind::Secondary => (
                if hovered {
                    Palette::BG_SELECTED()
                } else {
                    Palette::BG_CARD_HOVER()
                },
                Palette::TEXT_SECONDARY(),
            ),
            Kind::Danger => (
                if hovered {
                    Palette::BTN_DANGER_HOVER()
                } else {
                    Palette::BTN_DANGER_BG()
                },
                Palette::ERROR(),
            ),
            Kind::Ghost => (
                if hovered {
                    Palette::BG_CARD_HOVER()
                } else {
                    Color::TRANSPARENT
                },
                Palette::TEXT_DIMMER(),
            ),
        };
        let disabled = matches!(status, button::Status::Disabled);
        let dim = |color: Color| {
            if disabled {
                color.scale_alpha(0.5)
            } else {
                color
            }
        };
        button::Style {
            background: Some(dim(background).into()),
            text_color: dim(text_color),
            border: Border::default().rounded(6),
            ..Default::default()
        }
    }
}
