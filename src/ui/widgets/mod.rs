//! GitSpace widgets the kit does not have yet, written the way colony-ui's are.
//!
//! Every widget reads `Palette` and `Typography::current()` when it is drawn, so a change
//! of theme, accent, text size or font shows on the next frame. They can move to
//! colony-ui later without changes.

#[expect(
    dead_code,
    reason = "the repository screens will use the other buttons"
)]
pub mod button;
#[expect(dead_code, reason = "the repository header and lists will use it")]
pub mod chip;
pub mod menu;
#[expect(dead_code, reason = "the add-repository page will use it")]
pub mod segmented;
#[expect(dead_code, reason = "the repository tabs will use it")]
pub mod tabs;
pub mod toast;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// True when `line` holds `marker` followed by six hex digits.
    fn has_hex_after(line: &str, marker: &str) -> bool {
        line.match_indices(marker).any(|(at, _)| {
            let rest = &line[at + marker.len()..];
            rest.len() >= 6 && rest.chars().take(6).all(|c| c.is_ascii_hexdigit())
        })
    }

    #[test]
    fn ui_has_no_hex_colours() {
        let mut files = Vec::new();
        rust_files(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui"),
            &mut files,
        );

        let mut offenders = Vec::new();
        for file in files {
            // Ruling R2: theme.rs is the old palette bridge, deleted in Task 18.
            if file.file_name().is_some_and(|name| name == "theme.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&file).unwrap();
            for (number, line) in source.lines().enumerate() {
                // shortcut: everything after `#[cfg(test)]` counts as test code; test modules sit at the end of each file.
                if line.trim_start().starts_with("#[cfg(test)]") {
                    break;
                }
                if line.contains("Color::from_rgb")
                    || has_hex_after(line, "0x")
                    || has_hex_after(line, "#")
                {
                    offenders.push(format!(
                        "{}:{}: {}",
                        file.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "hex colours in the UI:\n{}",
            offenders.join("\n")
        );
    }
}
