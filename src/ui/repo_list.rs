//! The sidebar's repository list as data: which rows show, in which group and order.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::config::RepoEntry;
use crate::git::status::QuickStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: Arc<str>,
    /// The folder name.
    pub name: String,
    pub pinned: bool,
    /// `None` until the background scan has looked at the repository.
    pub status: Option<QuickStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub label_key: &'static str,
    pub rows: Vec<Row>,
}

/// With an empty `query`: pinned repositories by name, then the others by last opened
/// (never-opened ones last, by name). With a query: one group of the repositories whose
/// folder name contains it, ignoring case, in the same order. Hidden entries never show.
pub fn groups(
    entries: &[RepoEntry],
    statuses: &HashMap<String, QuickStatus>,
    query: &str,
) -> Vec<Group> {
    let query = query.trim();
    let mut rows: Vec<(Row, Option<i64>)> = entries
        .iter()
        .filter(|entry| !entry.hidden)
        .map(|entry| {
            let row = Row {
                path: Arc::from(entry.path.as_str()),
                name: folder_name(&entry.path),
                pinned: entry.pinned,
                status: statuses.get(&entry.path).cloned(),
            };
            (row, entry.last_opened)
        })
        .filter(|(row, _)| query.is_empty() || !match_ranges(&row.name, query).is_empty())
        .collect();
    rows.sort_by(|(a, a_opened), (b, b_opened)| {
        b.pinned
            .cmp(&a.pinned)
            // Pinned rows keep their name order; for the others the newest opening comes first.
            .then_with(|| match a.pinned {
                true => std::cmp::Ordering::Equal,
                false => b_opened.cmp(a_opened),
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
    });
    let rows: Vec<Row> = rows.into_iter().map(|(row, _)| row).collect();

    if !query.is_empty() {
        return vec![Group {
            label_key: "sidebar_results",
            rows,
        }];
    }
    let (pinned, recent): (Vec<Row>, Vec<Row>) = rows.into_iter().partition(|row| row.pinned);
    [("sidebar_pinned", pinned), ("sidebar_recent", recent)]
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(label_key, rows)| Group { label_key, rows })
        .collect()
}

/// Byte ranges of every non-overlapping occurrence of `query` in `name`, ignoring case.
pub fn match_ranges(name: &str, query: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    if query.is_empty() {
        return ranges;
    }
    let mut start = 0;
    while start < name.len() {
        match match_len(&name[start..], query) {
            Some(len) => {
                ranges.push(start..start + len);
                start += len;
            }
            None => start += name[start..].chars().next().map_or(1, char::len_utf8),
        }
    }
    ranges
}

/// The byte length of the prefix of `haystack` that equals `needle` ignoring case, if any.
/// Compared char by char, so a letter whose lower case has another byte length still lines up.
fn match_len(haystack: &str, needle: &str) -> Option<usize> {
    let mut chars = haystack.char_indices();
    for wanted in needle.chars() {
        let (_, found) = chars.next()?;
        if !found.to_lowercase().eq(wanted.to_lowercase()) {
            return None;
        }
    }
    Some(chars.next().map_or(haystack.len(), |(at, _)| at))
}

fn folder_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, pinned: bool, last_opened: Option<i64>) -> RepoEntry {
        RepoEntry {
            path: path.to_string(),
            pinned,
            last_opened,
            ..RepoEntry::default()
        }
    }

    fn layout(groups: &[Group]) -> Vec<(&str, Vec<&str>)> {
        groups
            .iter()
            .map(|group| {
                let names = group.rows.iter().map(|row| row.name.as_str()).collect();
                (group.label_key, names)
            })
            .collect()
    }

    #[test]
    fn groups_put_pinned_first_then_recent_by_last_opened() {
        let entries = [
            entry("/r/old", false, Some(10)),
            entry("/r/zeta", true, Some(1)),
            entry("/r/new", false, Some(30)),
            entry("/r/Alpha", true, Some(5)),
            entry("/r/mid", false, Some(20)),
        ];
        let mut statuses = HashMap::new();
        let dirty = QuickStatus {
            changed: 3,
            ..QuickStatus::default()
        };
        statuses.insert("/r/new".to_string(), dirty.clone());

        let groups = groups(&entries, &statuses, "");
        assert_eq!(
            layout(&groups),
            [
                ("sidebar_pinned", vec!["Alpha", "zeta"]),
                ("sidebar_recent", vec!["new", "mid", "old"]),
            ]
        );
        assert_eq!(&*groups[1].rows[0].path, "/r/new");
        assert_eq!(groups[1].rows[0].status, Some(dirty));
        assert_eq!(groups[1].rows[1].status, None);
    }

    #[test]
    fn never_opened_discovered_repos_sort_last_by_name() {
        let entries = [
            RepoEntry {
                discovered: true,
                ..entry("/c/beta", false, None)
            },
            entry("/r/opened", false, Some(1)),
            RepoEntry {
                discovered: true,
                ..entry("/c/Alpha", false, None)
            },
        ];
        assert_eq!(
            layout(&groups(&entries, &HashMap::new(), "")),
            [("sidebar_recent", vec!["opened", "Alpha", "beta"])]
        );
    }

    #[test]
    fn hidden_repos_are_not_listed() {
        let entries = [
            RepoEntry {
                discovered: true,
                hidden: true,
                ..entry("/c/gone", true, Some(9))
            },
            entry("/r/kept", false, Some(1)),
        ];
        assert_eq!(
            layout(&groups(&entries, &HashMap::new(), "")),
            [("sidebar_recent", vec!["kept"])]
        );
        assert!(groups(&entries, &HashMap::new(), "gone")[0].rows.is_empty());
    }

    #[test]
    fn query_filters_case_insensitively_into_one_group() {
        let entries = [
            entry("/r/Colony", false, Some(1)),
            entry("/r/colony-ui", true, Some(2)),
            entry("/r/GitSpace", false, Some(3)),
            entry("/r/Colony-Camera", false, None),
        ];
        assert_eq!(
            layout(&groups(&entries, &HashMap::new(), " COL ")),
            [(
                "sidebar_results",
                vec!["colony-ui", "Colony", "Colony-Camera"]
            )]
        );
    }

    #[test]
    fn match_ranges_find_every_occurrence() {
        assert_eq!(match_ranges("colony-Colony", "col"), [0..3, 7..10]);
        assert_eq!(match_ranges("aaaa", "aa"), [0..2, 2..4]);
        assert!(match_ranges("GitSpace", "x").is_empty());
        assert!(match_ranges("GitSpace", "").is_empty());
        // "É" is two bytes; the match after it still lands on char boundaries.
        assert_eq!(match_ranges("Été-été", "été"), [0..5, 6..11]);
    }
}
