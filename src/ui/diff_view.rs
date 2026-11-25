//! A patch drawn with line numbers: hunk headers on a tinted band, added lines on
//! `success_bg`, removed lines on `error_bg`. Changes and History share it.

use colony_ui::theme::Palette;
use colony_ui::Typography;
use iced::widget::{container, row, scrollable, text, Column};
use iced::{Color, Element, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// "\ No newline at end of file".
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// The line number before the change; `None` for an added line.
    pub old: Option<u32>,
    /// The line number after the change; `None` for a removed line.
    pub new: Option<u32>,
    /// The patch line, with its `+`, `-` or space prefix.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ …` line.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// Splits a unified patch into hunks and numbers their lines. File headers before the
/// first hunk, and any line that is not part of a hunk, are left out.
pub fn parse_hunks(patch: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let (mut old, mut new) = (0, 0);
    for line in patch.lines() {
        if line.starts_with("@@") {
            (old, new) = hunk_starts(line);
            hunks.push(Hunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = hunks.last_mut() else {
            continue;
        };
        let (kind, old_number, new_number) = match line.chars().next() {
            Some(' ') => (LineKind::Context, Some(old), Some(new)),
            Some('+') => (LineKind::Added, None, Some(new)),
            Some('-') => (LineKind::Removed, Some(old), None),
            Some('\\') => (LineKind::Note, None, None),
            _ => continue,
        };
        old += u32::from(old_number.is_some());
        new += u32::from(new_number.is_some());
        hunk.lines.push(DiffLine {
            kind,
            old: old_number,
            new: new_number,
            text: line.to_string(),
        });
    }
    hunks
}

/// The first old and new line numbers of a `@@ -a,b +c,d @@` header.
fn hunk_starts(header: &str) -> (u32, u32) {
    let start = |sign: char| {
        header
            .split_whitespace()
            .find_map(|part| part.strip_prefix(sign))
            .and_then(|range| range.split(',').next())
            .and_then(|number| number.parse().ok())
            .unwrap_or(1)
    };
    (start('-'), start('+'))
}

/// The hunks in a vertical scroll area that fills its parent.
pub fn view<'a, M: 'a>(hunks: &'a [Hunk]) -> Element<'a, M> {
    let typo = Typography::current();
    let size = typo.sz(12);
    let mut lines = Column::new().padding([8, 0]);
    for hunk in hunks {
        lines = lines.push(
            container(
                text(hunk.header.as_str())
                    .size(size)
                    .font(typo.regular)
                    .color(Palette::ACCENT()),
            )
            .width(Length::Fill)
            .padding([2, 16])
            .style(|_| band(Palette::ACCENT().scale_alpha(0.12))),
        );
        for line in &hunk.lines {
            let (background, color) = match line.kind {
                LineKind::Added => (Palette::SUCCESS_BG(), Palette::SUCCESS()),
                LineKind::Removed => (Palette::ERROR_BG(), Palette::ERROR()),
                LineKind::Context => (Color::TRANSPARENT, Palette::TEXT_DIM()),
                LineKind::Note => (Color::TRANSPARENT, Palette::TEXT_DIMMEST()),
            };
            // One column, as in the mockup: the new number, or the old one for a removal.
            let number = line
                .new
                .or(line.old)
                .map(|n| n.to_string())
                .unwrap_or_default();
            lines = lines.push(
                container(
                    row![
                        text(number)
                            .size(size)
                            .font(typo.regular)
                            .color(Palette::TEXT_DIMMEST())
                            .width(Length::Fixed(42.0))
                            .align_x(iced::alignment::Horizontal::Right),
                        text(line.text.as_str())
                            .size(size)
                            .font(typo.regular)
                            .color(color),
                    ]
                    .spacing(8),
                )
                .width(Length::Fill)
                .padding([0, 16])
                .style(move |_| band(background)),
            );
        }
    }
    scrollable(lines)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn band(background: Color) -> container::Style {
    container::Style {
        background: Some(background.into()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hunks_numbers_lines() {
        let patch = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n\
            @@ -40,4 +40,5 @@ fn set_active_theme\n a\n-b\n+c\n+d\n e\n\
            \\ No newline at end of file\n@@ -90 +91,2 @@\n z\n+y\n";
        let hunks = parse_hunks(patch);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].header, "@@ -40,4 +40,5 @@ fn set_active_theme");

        let numbers: Vec<_> = hunks[0]
            .lines
            .iter()
            .map(|line| (line.kind, line.old, line.new, line.text.as_str()))
            .collect();
        assert_eq!(
            numbers,
            [
                (LineKind::Context, Some(40), Some(40), " a"),
                (LineKind::Removed, Some(41), None, "-b"),
                (LineKind::Added, None, Some(41), "+c"),
                (LineKind::Added, None, Some(42), "+d"),
                (LineKind::Context, Some(42), Some(43), " e"),
                (LineKind::Note, None, None, "\\ No newline at end of file"),
            ]
        );
        // A range without a count still gives the start line.
        assert_eq!(hunks[1].lines[0].old, Some(90));
        assert_eq!(hunks[1].lines[1].new, Some(92));
        // No hunk: a binary file, or the truncation marker on its own.
        assert!(parse_hunks("Binary file change\n").is_empty());
    }
}
