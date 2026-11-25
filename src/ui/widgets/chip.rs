//! Small rounded labels: branch names, ahead/behind counts, ref badges.

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::widget::{container, text};
use iced::{Border, Color, Element};

/// What a chip's text colour says about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
    /// Remote branches.
    Remote,
    /// Tags.
    Tag,
}

impl Tone {
    /// The palette role of the chip's text, read from the active theme.
    pub fn color(self) -> Color {
        match self {
            Tone::Neutral => Palette::TEXT_SECONDARY(),
            Tone::Accent => Palette::ACCENT(),
            Tone::Success => Palette::SUCCESS(),
            Tone::Warning | Tone::Tag => Palette::WARNING(),
            Tone::Error => Palette::ERROR(),
            Tone::Remote => Palette::ACCENT_ICON(),
        }
    }
}

pub fn chip<'a, M: 'a>(label: &str, tone: Tone) -> Element<'a, M> {
    let typo = Typography::current();
    container(
        text(label.to_string())
            .size(typo.sz(11))
            .font(typo.regular)
            .color(tone.color()),
    )
    .padding([3, 9])
    .style(|_| container::Style {
        background: Some(Palette::BG_CARD_HOVER().into()),
        border: Border::default().rounded(20),
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_maps_to_palette_roles() {
        // The tests never change the active theme, so this is the default one.
        assert_eq!(Tone::Remote.color(), Palette::ACCENT_ICON());
        assert_eq!(Tone::Tag.color(), Palette::WARNING());
        assert_eq!(Tone::Success.color(), Palette::SUCCESS());
        assert_eq!(Tone::Error.color(), Palette::ERROR());
        assert_eq!(Tone::Accent.color(), Palette::ACCENT());
        assert_eq!(Tone::Neutral.color(), Palette::TEXT_SECONDARY());
    }
}
