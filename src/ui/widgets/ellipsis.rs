//! One line of text that ends in "…" when the space it gets is too narrow for all of it.
//!
//! A row lays it out last, with the width the other widgets (badges, a hash) leave, like a
//! `Length::Fill` child, so they keep their size and the text gives way. It is only as wide
//! as the text it shows, so the widgets after it in the row follow the text.

use iced::advanced::layout::{self, Layout};
use iced::advanced::text::paragraph::Plain;
use iced::advanced::text::{self, Paragraph as _};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, renderer};
use iced::alignment::Vertical;
use iced::{Color, Element, Font, Length, Rectangle, Renderer, Size, Theme};

const ELLIPSIS: &str = "\u{2026}";

type Paragraph = <Renderer as text::Renderer>::Paragraph;

/// `content` on one line in `font` at `size`, cut with "…" to the width it gets.
pub fn ellipsis<'a, M: 'a>(
    content: impl Into<String>,
    size: f32,
    font: Font,
    color: Color,
) -> Element<'a, M> {
    Element::new(Ellipsis {
        content: content.into(),
        size,
        font,
        color,
    })
}

struct Ellipsis {
    content: String,
    size: f32,
    font: Font,
    color: Color,
}

#[derive(Default)]
struct State {
    /// All of the text, measured once per change of text, size or font.
    full: Plain<Paragraph>,
    /// The text drawn: all of it, or its cut start and "…".
    shown: Plain<Paragraph>,
    /// The width `shown` was cut for.
    width: f32,
}

impl Ellipsis {
    fn text<'b>(&self, content: &'b str) -> text::Text<&'b str, Font> {
        text::Text {
            content,
            bounds: Size::INFINITE,
            size: self.size.into(),
            line_height: text::LineHeight::default(),
            font: self.font,
            align_x: text::Alignment::Default,
            align_y: Vertical::Top,
            shaping: text::Shaping::default(),
            wrapping: text::Wrapping::None,
        }
    }
}

impl<M> Widget<M, Theme, Renderer> for Ellipsis {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let width = limits.max().width;
        // Each try shapes text again, so the cut is only searched when the text, its format
        // or the width changed.
        if state.full.update(self.text(&self.content)) || state.width != width {
            state.width = width;
            let full = state.full.min_width();
            let shown = fit(&self.content, width, |candidate| {
                if candidate == self.content {
                    full
                } else {
                    Paragraph::with_text(self.text(candidate)).min_width()
                }
            });
            state.shown.update(self.text(&shown));
        }
        let shown = state.shown.min_bounds();
        layout::Node::new(Size::new(
            shown.width.min(width),
            shown.height.min(limits.max().height),
        ))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // Clipped to its own bounds: even "…" alone can be wider than a tiny space.
        let Some(clip) = layout.bounds().intersection(viewport) else {
            return;
        };
        let state = tree.state.downcast_ref::<State>();
        widget::text::draw(
            renderer,
            style,
            layout.bounds(),
            state.shown.raw(),
            widget::text::Style {
                color: Some(self.color),
            },
            &clip,
        );
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.text(None, layout.bounds(), &self.content);
    }
}

/// `content` when `width_of` it fits in `max_width`, or else the longest start of it, without
/// trailing spaces, that fits with "…" after it. Only "…" is left when nothing fits.
fn fit(content: &str, max_width: f32, width_of: impl Fn(&str) -> f32) -> String {
    if content.is_empty() || width_of(content) <= max_width {
        return content.to_string();
    }
    let starts: Vec<usize> = content.char_indices().map(|(at, _)| at).collect();
    let cut = |chars: usize| format!("{}{ELLIPSIS}", content[..starts[chars]].trim_end());
    // Binary search between a char count known to fit (or zero) and one known not to.
    let (mut fits, mut too_wide) = (0, starts.len());
    while too_wide - fits > 1 {
        let middle = (fits + too_wide) / 2;
        if width_of(&cut(middle)) <= max_width {
            fits = middle;
        } else {
            too_wide = middle;
        }
    }
    cut(fits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten pixels per character.
    fn width(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn a_summary_too_wide_for_its_space_ends_in_an_ellipsis() {
        // "feat: add file 3" is 160 px wide.
        assert_eq!(fit("feat: add file 3", 160.0, width), "feat: add file 3");
        // The space before the cut is dropped, not drawn before "…".
        assert_eq!(fit("feat: add file 3", 159.0, width), "feat: add file…");
        assert_eq!(fit("feat: add file 3", 100.0, width), "feat: add…");
        assert_eq!(fit("feat: add file 3", 99.0, width), "feat: ad…");
        assert_eq!(fit("feat: add file 3", 5.0, width), "…");
        assert_eq!(fit("", 0.0, width), "");
        // Cut on character boundaries, never inside one.
        assert_eq!(fit("été très chaud", 60.0, width), "été t…");
    }
}
