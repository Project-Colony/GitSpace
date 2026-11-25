//! GitSpace's own labels in English and French.
//!
//! Kit labels (theme names, accents) come from `colony_ui::i18n`; this table
//! holds GitSpace's vocabulary. The active locale is the one set with
//! `colony_ui::i18n::set_locale`, so both tables switch together.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use colony_ui::i18n::Locale;

const EN: &str = include_str!("i18n/en.json");
const FR: &str = include_str!("i18n/fr.json");

/// Looks up a label in the active locale; falls back to English, then to the key.
pub fn t(key: &str) -> &'static str {
    lookup(colony_ui::i18n::locale(), key)
}

/// Looks up a label and replaces each `{name}` placeholder with its value.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    fill(t(key), args)
}

fn fill(label: &str, args: &[(&str, &str)]) -> String {
    args.iter().fold(label.to_owned(), |out, (name, value)| {
        out.replace(&format!("{{{name}}}"), value)
    })
}

fn table(locale: Locale) -> &'static HashMap<String, String> {
    static EN_TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    static FR_TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    let (cell, source) = match locale {
        Locale::En => (&EN_TABLE, EN),
        Locale::Fr => (&FR_TABLE, FR),
    };
    // Both files are embedded and parsed by the tests below, so this cannot fail in a tested build.
    cell.get_or_init(|| serde_json::from_str(source).expect("embedded label table is valid JSON"))
}

fn lookup(locale: Locale, key: &str) -> &'static str {
    table(locale)
        .get(key)
        .or_else(|| table(Locale::En).get(key))
        .map(String::as_str)
        .unwrap_or_else(|| intern(key))
}

/// Shows an unknown key as itself. Interned so a view redrawn every frame leaks it only once.
fn intern(key: &str) -> &'static str {
    static UNKNOWN: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut unknown = UNKNOWN.get_or_init(Default::default).lock().unwrap();
    if let Some(found) = unknown.get(key) {
        return found;
    }
    let leaked: &'static str = Box::leak(key.into());
    unknown.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(label: &str) -> HashSet<&str> {
        label
            .split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
            .collect()
    }

    #[test]
    fn en_and_fr_have_identical_key_sets() {
        let en: HashSet<_> = table(Locale::En).keys().collect();
        let fr: HashSet<_> = table(Locale::Fr).keys().collect();
        assert_eq!(
            en,
            fr,
            "missing on one side: {:?}",
            en.symmetric_difference(&fr)
        );
    }

    #[test]
    fn placeholders_match_between_locales() {
        let fr = table(Locale::Fr);
        for (key, en_label) in table(Locale::En) {
            let fr_label = fr.get(key).map(String::as_str).unwrap_or_default();
            assert_eq!(placeholders(en_label), placeholders(fr_label), "key {key}");
        }
    }

    #[test]
    fn lookups_fill_placeholders_and_fall_back_to_the_key() {
        assert_eq!(
            lookup(Locale::Fr, "startup_migration_failed"),
            table(Locale::Fr)["startup_migration_failed"]
        );
        assert_eq!(lookup(Locale::Fr, "no_such_key"), "no_such_key");
        assert_eq!(
            fill(
                "Opened {repo} on {branch}",
                &[("repo", "GitSpace"), ("branch", "main")]
            ),
            "Opened GitSpace on main"
        );
    }

    #[test]
    fn ui_has_no_literal_strings() {
        let mut files = Vec::new();
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            }
        }

        let mut offenders = Vec::new();
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            for (number, line) in crate::ui::non_test_lines(&source) {
                if line.contains("// i18n: not user-visible") {
                    continue;
                }
                let literal = ["text(\"", "button(\""].iter().any(|call| {
                    line.match_indices(call).any(|(at, _)| {
                        line[at + call.len()..]
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_alphabetic())
                    })
                });
                if literal {
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
            "literal UI strings:\n{}",
            offenders.join("\n")
        );
    }
}
