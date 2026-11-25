//! A strip of tabs with an optional count after each label.

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::widget::{button, row, text, Row};
use iced::{Alignment, Border, Color, Element};

/// `items` are labels with an optional count, shown dimmed after the label.
pub fn tabs<'a, M: Clone + 'a>(
    items: &[(String, Option<usize>)],
    selected: usize,
    on_select: impl Fn(usize) -> M,
) -> Element<'a, M> {
    let typo = Typography::current();
    let tabs = items.iter().enumerate().map(|(index, (label, count))| {
        let is_selected = index == selected;
        let mut content = row![text(label.clone()).size(typo.sz(12)).font(typo.regular)]
            .spacing(6)
            .align_y(Alignment::Center);
        if let Some(count) = count {
            content = content.push(
                text(count.to_string())
                    .size(typo.sz(12))
                    .font(typo.regular)
                    .color(Palette::TEXT_DIMMEST()),
            );
        }
        button(content)
            .on_press(on_select(index))
            .padding([6, 12])
            .style(move |_theme, status| {
                let (background, text_color) = match status {
                    _ if is_selected => (Palette::BG_SELECTED(), Palette::TEXT_PRIMARY()),
                    button::Status::Hovered | button::Status::Pressed => {
                        (Palette::BG_CARD_HOVER(), Palette::TEXT_MUTED())
                    }
                    _ => (Color::TRANSPARENT, Palette::TEXT_MUTED()),
                };
                button::Style {
                    background: Some(background.into()),
                    text_color,
                    border: Border::default().rounded(6),
                    ..Default::default()
                }
            })
            .into()
    });
    Row::with_children(tabs).spacing(4).into()
}
