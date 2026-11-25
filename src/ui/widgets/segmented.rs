//! A row of joined buttons where exactly one is selected.

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::widget::{button, container, text, Row};
use iced::{border, Border, Element};

pub fn segmented<'a, M: Clone + 'a>(
    items: &[String],
    selected: usize,
    on_select: impl Fn(usize) -> M,
) -> Element<'a, M> {
    let typo = Typography::current();
    let last = items.len().saturating_sub(1);
    let segments = items.iter().enumerate().map(|(index, label)| {
        let is_selected = index == selected;
        // Only the outer corners are rounded; the 1 px gaps show the frame as dividers.
        let mut radius = border::radius(0);
        if index == 0 {
            radius = radius.left(7);
        }
        if index == last {
            radius = radius.right(7);
        }
        button(text(label.clone()).size(typo.sz(12)).font(typo.regular))
            .on_press(on_select(index))
            .padding([7, 14])
            .style(move |_theme, status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let (background, text_color) = if is_selected {
                    (Palette::BG_SELECTED(), Palette::TEXT_PRIMARY())
                } else if hovered {
                    (Palette::BG_CARD_HOVER(), Palette::TEXT_MUTED())
                } else {
                    (Palette::BG_SIDEBAR(), Palette::TEXT_MUTED())
                };
                button::Style {
                    background: Some(background.into()),
                    text_color,
                    border: Border::default().rounded(radius),
                    ..Default::default()
                }
            })
            .into()
    });
    container(Row::with_children(segments).spacing(1))
        .padding(1)
        .style(|_| container::Style {
            background: Some(Palette::BG_SELECTED().into()),
            border: Border::default().rounded(8),
            ..Default::default()
        })
        .into()
}
