pub mod add_repo;
pub mod app;
pub mod branches;
pub mod changes;
pub mod context;
pub mod diff_view;
pub mod history;
pub mod preferences;
pub mod repo_header;
pub mod repo_list;
pub mod sidebar;
pub mod stashes;
pub mod widgets;

/// The lines of `source`, numbered from zero, without the `#[cfg(test)]` items: a whole
/// `mod tests { … }` or a single test-only function, `use` or field. The source guards scan
/// what is left.
#[cfg(test)]
pub(crate) fn non_test_lines(source: &str) -> Vec<(usize, &str)> {
    let mut kept = Vec::new();
    let (mut skipping, mut opened, mut depth) = (false, false, 0i64);
    for (number, line) in source.lines().enumerate() {
        if !skipping && line.trim_start().starts_with("#[cfg(test)]") {
            (skipping, opened, depth) = (true, false, 0);
        }
        if !skipping {
            kept.push((number, line));
            continue;
        }
        // shortcut: brackets inside string literals and comments count too; good enough while
        // test items keep them balanced, a real tokenizer if one ever does not.
        let mut code = line.to_string();
        for quoted in ["'{'", "'}'", "'('", "')'", "'['", "']'"] {
            code = code.replace(quoted, "");
        }
        let count = |set: &[char]| code.matches(set).count() as i64;
        depth += count(&['{', '(', '[']) - count(&['}', ')', ']']);
        opened |= code.contains('{');
        // The item ends where its body closes, or at the `;` or `,` of an item without one.
        if depth <= 0 && (opened || code.trim_end().ends_with([';', ','])) {
            skipping = false;
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::non_test_lines;

    #[test]
    fn source_guards_skip_only_test_items() {
        let sample = "fn a() {}\n\
                      #[cfg(test)]\n\
                      fn helper(\n\
                      \x20   a: u8,\n\
                      ) -> char {\n\
                      \x20   '{'\n\
                      }\n\
                      fn b() {}\n\
                      #[cfg(test)]\n\
                      use std::fmt;\n\
                      fn c() {}\n\
                      #[cfg(test)]\n\
                      mod tests {\n\
                      \x20   fn d() {}\n\
                      }\n\
                      fn e() {}";
        let kept = non_test_lines(sample);
        assert_eq!(
            kept,
            [
                (0, "fn a() {}"),
                (7, "fn b() {}"),
                (10, "fn c() {}"),
                (15, "fn e() {}")
            ]
        );
    }
}
