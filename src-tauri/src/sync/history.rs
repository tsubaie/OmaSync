//! History and undo, read from the git log in plain words.

use super::*;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub sha: String,
    pub computer: String,
    pub when: i64,
    /// Plain names of what changed, e.g. "Keyboard shortcuts".
    pub labels: Vec<String>,
    pub is_undo: bool,
    /// Where two computers' changes were joined after a clash; it can't be undone on its own.
    pub is_merge: bool,
    /// The very first save, which has nothing before it to go back to.
    pub first: bool,
    /// Every file it changed: what Undo would put back.
    pub files: Vec<Change>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Change {
    /// The path under home, or DATA_LABEL for the shared choices.
    pub path: String,
    /// "Added", "Changed" or "Removed".
    pub how: &'static str,
}

/// Reads `git ... --name-status` lines ("M\tpath", "R100\told\tnew") as changes, skipping
/// the ignore rules, which aren't anyone's settings.
pub(crate) fn parse_changes<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<Change> {
    lines
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let status = parts.next()?;
            let file = parts.next_back()?;
            let how = match status.chars().next()? {
                'A' => "Added",
                'D' => "Removed",
                _ => "Changed",
            };
            (!file.starts_with(".chezmoiignore")).then(|| Change {
                path: source_to_target(file),
                how,
            })
        })
        .collect()
}

/// A commit message body naming every change in plain words, grouped like the app shows them.
pub(crate) fn describe(changes: &[Change]) -> String {
    let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
    for c in changes {
        let name = label(&c.path);
        let file = c.path.rsplit('/').next().unwrap_or(&c.path);
        let item = if c.path == DATA_LABEL {
            "updated".to_string()
        } else {
            format!("{} {file}", c.how.to_lowercase())
        };
        match groups.iter_mut().find(|(n, _)| *n == name) {
            Some((_, items)) => items.push(item),
            None => groups.push((name, vec![item])),
        }
    }
    groups
        .iter()
        .map(|(name, items)| format!("{name}: {}", items.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

const UNDO_PREFIX: &str = "Undo: ";

pub fn history(ctx: &Ctx) -> Vec<Entry> {
    let log = ctx
        .git(&[
            "log",
            "-n",
            "50",
            "--name-status",
            "--format=%x1e%H%x09%an%x09%ct%x09%s%x09%P",
        ])
        .unwrap_or_default();
    parse_history(&log)
}

/// Parses `git log --name-status --format=%x1e%H%x09%an%x09%ct%x09%s%x09%P`.
pub fn parse_history(log: &str) -> Vec<Entry> {
    log.split('\x1e')
        .filter_map(|block| {
            let mut lines = block.lines();
            let head: Vec<&str> = lines.next()?.split('\t').collect();
            let [sha, computer, when, subject, parents] = head[..] else {
                return None;
            };
            let files = parse_changes(lines.filter(|l| !l.is_empty()));
            let mut labels: Vec<String> = Vec::new();
            for c in &files {
                let name = label(&c.path).to_string();
                if !labels.contains(&name) {
                    labels.push(name);
                }
            }
            Some(Entry {
                sha: sha.into(),
                computer: computer.into(),
                when: when.parse().ok()?,
                labels,
                is_undo: subject.starts_with(UNDO_PREFIX),
                is_merge: parents.contains(' '),
                first: parents.is_empty(),
                files,
            })
        })
        .collect()
}

/// chezmoi's source name for a file back to its path under home (dot_config -> .config).
pub fn source_to_target(path: &str) -> String {
    if path == ".chezmoidata.yaml" {
        return DATA_LABEL.into();
    }
    if path.starts_with(".background/") {
        return path.into();
    }
    path.split('/')
        .map(|part| {
            let p = strip_attributes(part);
            p.strip_prefix("dot_")
                .map_or(p.clone(), |r| format!(".{r}"))
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Reverses one earlier change. The caller then syncs, which applies it here and shares it.
pub fn undo(ctx: &Ctx, sha: &str) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    save_local(ctx, &|_| {})?;
    let synced_before = ctx.load_data().active_folders(&ctx.host);
    if ctx.git(&["revert", "--no-commit", sha]).is_err() {
        let _ = ctx.git(&["revert", "--abort"]);
        return Err("can't undo: later changes depend on it".into());
    }
    // Never puts back a file that's switched off now (the choices as undone decide): it would
    // reach every computer, over copies that were never saved.
    let choices = ctx.load_data();
    let staged = ctx.git(&["diff", "--cached", "--name-status"])?;
    let revives_switched_off = parse_changes(staged.lines()).iter().any(|c| {
        c.how == "Added"
            && c.path != DATA_LABEL
            && !c.path.starts_with(".background/")
            && !choices.syncs(&ctx.host, &c.path)
    });
    if revives_switched_off {
        let _ = ctx.git(&["revert", "--abort"]);
        return Err(
            "This change would put back settings that are switched off, so it can't be undone."
                .into(),
        );
    }
    let author = ctx
        .git(&["log", "-1", "--format=%an", sha])
        .unwrap_or_default();
    let author = author.trim();
    // Commits are authored by the computer's ID; the message says its name.
    let who = choices
        .computers
        .get(&choices.resolve(author))
        .map(|c| c.name.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or(author);
    ctx.git(&[
        "commit",
        "-q",
        "-m",
        &format!("{UNDO_PREFIX}a change from {who}"),
    ])?;
    save_newly_synced(ctx, &synced_before)?;
    // Put the undone files in place now; otherwise the next sync would re-save the old ones over the undo.
    apply_chezmoi(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_reads_git_log_in_plain_words() {
        assert_eq!(
            source_to_target("private_dot_config/omarchy/private_shell.json"),
            ".config/omarchy/shell.json"
        );
        assert_eq!(
            source_to_target("dot_local/share/applications/x.desktop"),
            ".local/share/applications/x.desktop"
        );
        let log = "\x1eaaa\tlaptop\t200\tUndo: a change from desk\tbbb\n\nM\tdot_config/hypr/bindings.lua\nM\t.chezmoidata.yaml\n\x1ebbb\tdesk\t100\tSync from desk\t\n\nA\tdot_config/hypr/input.lua\nD\tdot_config/hypr/bindings.lua\n";
        let h = parse_history(log);
        assert_eq!(h.len(), 2);
        assert_eq!(
            h[0].labels,
            vec!["Keyboard shortcuts".to_string(), DATA_LABEL.to_string()]
        );
        assert!(h[0].is_undo && !h[0].first);
        assert_eq!(
            (
                h[1].computer.as_str(),
                h[1].when,
                h[1].first,
                h[1].labels.len()
            ),
            ("desk", 100, true, 2)
        );
        assert_eq!(
            h[1].files,
            vec![
                Change {
                    path: ".config/hypr/input.lua".into(),
                    how: "Added"
                },
                Change {
                    path: ".config/hypr/bindings.lua".into(),
                    how: "Removed"
                },
            ]
        );
        assert_eq!(
            describe(&h[0].files),
            format!("Keyboard shortcuts: changed bindings.lua\n{DATA_LABEL}: updated")
        );
    }
}
