//! Popup menus: the "⋯" menus and right-click menus.
//!
//! The menu is an iced overlay drawn below its anchor, flipped above or moved left when
//! it would leave the window. A click outside it or Escape sends `on_close`; choosing an
//! item sends only that item's message, so the caller closes the menu when handling it.

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{layout, overlay, renderer, Clipboard, Layout, Shell};
use iced::widget::{button, container, row, text, Column, Space};
use iced::{
    keyboard, mouse, Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Shadow,
    Size, Theme, Vector,
};

/// Space between the anchor and the menu.
const GAP: f32 = 4.0;

pub struct MenuItem<M> {
    pub label: String,
    pub shortcut: Option<String>,
    pub on_select: M,
    /// Drawn in the error colour; the action still asks for confirmation.
    pub danger: bool,
}

pub enum Entry<M> {
    Item(MenuItem<M>),
    Separator,
}

/// Draws `anchor`, and the menu over everything else while `open` is true.
pub fn popup<'a, M: Clone + 'a>(
    anchor: Element<'a, M>,
    open: bool,
    entries: Vec<Entry<M>>,
    on_close: M,
) -> Element<'a, M> {
    Element::new(Popup {
        anchor,
        menu: open.then(|| list(entries)),
        on_close,
    })
}

fn list<'a, M: Clone + 'a>(entries: Vec<Entry<M>>) -> Element<'a, M> {
    let typo = Typography::current();
    let rows = entries.into_iter().map(|entry| -> Element<'a, M> {
        match entry {
            Entry::Separator => container(
                container(Space::new().width(Length::Fill).height(1)).style(|_| container::Style {
                    background: Some(Palette::DIVIDER().into()),
                    ..Default::default()
                }),
            )
            .padding([5, 2])
            .into(),
            Entry::Item(item) => {
                let color = if item.danger {
                    Palette::ERROR()
                } else {
                    Palette::TEXT_SECONDARY()
                };
                let mut content = row![
                    text(item.label)
                        .size(typo.sz(12))
                        .font(typo.regular)
                        .color(color),
                    Space::new().width(Length::Fill),
                ];
                if let Some(shortcut) = item.shortcut {
                    content = content.push(
                        text(shortcut)
                            .size(typo.sz(11))
                            .font(typo.regular)
                            .color(Palette::TEXT_DIMMEST()),
                    );
                }
                button(content.spacing(12))
                    .on_press(item.on_select)
                    .width(Length::Fill)
                    .padding([7, 10])
                    .style(|_theme, status| button::Style {
                        background: matches!(
                            status,
                            button::Status::Hovered | button::Status::Pressed
                        )
                        .then(|| Palette::BG_CARD_HOVER().into()),
                        border: Border::default().rounded(5),
                        ..Default::default()
                    })
                    .into()
            }
        }
    });
    container(Column::with_children(rows).width(230))
        .padding(6)
        .style(|_| container::Style {
            background: Some(Palette::BG_SIDEBAR().into()),
            border: Border::default()
                .rounded(8)
                .width(1)
                .color(Palette::BG_SELECTED()),
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(0.45),
                offset: Vector::new(0.0, 10.0),
                blur_radius: 30.0,
            },
            ..Default::default()
        })
        .into()
}

struct Popup<'a, M> {
    anchor: Element<'a, M>,
    menu: Option<Element<'a, M>>,
    on_close: M,
}

impl<M: Clone> Widget<M, Theme, Renderer> for Popup<'_, M> {
    fn children(&self) -> Vec<Tree> {
        std::iter::once(&self.anchor)
            .chain(&self.menu)
            .map(Tree::new)
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        match &self.menu {
            Some(menu) => tree.diff_children(&[self.anchor.as_widget(), menu.as_widget()]),
            None => tree.diff_children(&[self.anchor.as_widget()]),
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn size(&self) -> Size<Length> {
        self.anchor.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.anchor.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.anchor
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        self.anchor.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.anchor.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.anchor.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.anchor
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let (anchor_tree, rest) = tree.children.split_at_mut(1);
        match (&mut self.menu, rest.first_mut()) {
            (Some(menu), Some(menu_tree)) => Some(overlay::Element::new(Box::new(Overlay {
                menu,
                tree: menu_tree,
                anchor: layout.bounds() + translation,
                on_close: self.on_close.clone(),
            }))),
            // The anchor's own overlays (a tooltip) only show while the menu is closed.
            _ => self.anchor.as_widget_mut().overlay(
                &mut anchor_tree[0],
                layout,
                renderer,
                viewport,
                translation,
            ),
        }
    }
}

struct Overlay<'a, 'b, M> {
    menu: &'b mut Element<'a, M>,
    tree: &'b mut Tree,
    anchor: Rectangle,
    on_close: M,
}

impl<M: Clone> overlay::Overlay<M, Theme, Renderer> for Overlay<'_, '_, M> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.menu.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let size = node.size();
        node.move_to(place(self.anchor, size, bounds))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let outside_click = matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
            && !cursor.is_over(layout.bounds());
        let escape = matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
        );
        if outside_click || escape {
            shell.publish(self.on_close.clone());
            shell.capture_event();
            return;
        }
        self.menu.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.menu.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }
}

/// Below the anchor and left-aligned with it when that fits the window; otherwise
/// right-aligned and/or above it, never past the top-left corner.
fn place(anchor: Rectangle, menu: Size, window: Size) -> Point {
    let x = if anchor.x + menu.width <= window.width {
        anchor.x
    } else {
        anchor.x + anchor.width - menu.width
    };
    let below = anchor.y + anchor.height + GAP;
    let y = if below + menu.height <= window.height {
        below
    } else {
        anchor.y - GAP - menu.height
    };
    Point::new(x.max(0.0), y.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_stays_inside_the_window() {
        let window = Size::new(800.0, 600.0);
        let menu = Size::new(230.0, 200.0);

        let top_left = Rectangle::new(Point::new(10.0, 10.0), Size::new(20.0, 20.0));
        assert_eq!(place(top_left, menu, window), Point::new(10.0, 34.0));

        let bottom_right = Rectangle::new(Point::new(760.0, 560.0), Size::new(30.0, 20.0));
        assert_eq!(place(bottom_right, menu, window), Point::new(560.0, 356.0));
    }
}
