//! Keeps the documentation in one piece: every relative link in the Markdown files points at a file
//! and a heading that exist, `docs/Home.md` links every other page under `docs/`, and the README
//! links `docs/Home.md`. A wiki generated from `docs/` is planned, and a broken link here would be
//! a broken page there.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every Markdown file under `dir`, recursively.
fn markdown_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn docs_pages() -> Vec<PathBuf> {
    let mut pages = Vec::new();
    markdown_under(&root().join("docs"), &mut pages);
    pages.sort();
    pages
}

/// The files whose links are checked: the README, AGENTS.md and everything in `docs/`.
fn checked_files() -> Vec<PathBuf> {
    let mut files = vec![root().join("README.md"), root().join("AGENTS.md")];
    files.extend(docs_pages());
    files
}

/// The lines of `text` outside fenced code blocks.
fn prose(text: &str) -> impl Iterator<Item = &str> {
    let mut fenced = false;
    text.lines().filter(move |line| {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            return false;
        }
        !fenced
    })
}

/// `line` without its inline code spans, whose contents are not links.
fn without_code_spans(line: &str) -> String {
    line.split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, part)| part)
        .collect()
}

/// The targets of the inline links `[text](target)` and images in `text`, leaving out code.
fn links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in prose(text) {
        let line = without_code_spans(line);
        let mut rest = line.as_str();
        while let Some(i) = rest.find("](") {
            rest = &rest[i + 2..];
            let Some(end) = rest.find(')') else { break };
            // A link may carry a title after a space: `(target "title")`.
            let target = rest[..end].split_whitespace().next().unwrap_or("");
            if !target.is_empty() {
                out.push(target.to_string());
            }
            rest = &rest[end..];
        }
    }
    out
}

/// The anchors GitHub makes for the headings in `text`: lower case, spaces as hyphens, most
/// punctuation dropped, and `-1`, `-2` for a heading that appears again.
fn anchors(text: &str) -> BTreeSet<String> {
    let mut seen: Vec<String> = Vec::new();
    for line in prose(text) {
        let hashes = line.chars().take_while(|&c| c == '#').count();
        if !(1..=6).contains(&hashes) || !line[hashes..].starts_with(' ') {
            continue;
        }
        let slug: String = line[hashes..]
            .trim()
            .trim_end_matches('#')
            .trim()
            .to_lowercase()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                '-' | '_' => Some(c),
                c if c.is_alphanumeric() => Some(c),
                _ => None,
            })
            .collect();
        seen.push(slug);
    }
    // Number the repeats after collecting, so `a`, `a` becomes `a`, `a-1`.
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut out = BTreeSet::new();
    for slug in seen {
        match counts.iter_mut().find(|(s, _)| *s == slug) {
            Some((_, n)) => {
                out.insert(format!("{slug}-{n}"));
                *n += 1;
            }
            None => {
                out.insert(slug.clone());
                counts.push((slug, 1));
            }
        }
    }
    out
}

/// Where a relative link in `file` points, or why it is broken. `None` for external links.
fn resolve(file: &Path, target: &str) -> Option<Result<PathBuf, String>> {
    if ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| target.starts_with(p))
    {
        return None;
    }
    let (path, anchor) = target.split_once('#').unwrap_or((target, ""));
    let resolved = if path.is_empty() {
        file.to_path_buf()
    } else {
        file.parent().unwrap().join(path)
    };
    let Ok(resolved) = resolved.canonicalize() else {
        return Some(Err(format!("{target}: no such file")));
    };
    if !anchor.is_empty() && resolved.extension().is_some_and(|e| e == "md") {
        let text = fs::read_to_string(&resolved).unwrap();
        if !anchors(&text).contains(&anchor.to_lowercase()) {
            return Some(Err(format!("{target}: no heading #{anchor} in that page")));
        }
    }
    Some(Ok(resolved))
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root().canonicalize().unwrap())
        .unwrap_or(path)
        .display()
        .to_string()
}

#[test]
fn every_relative_link_points_at_a_file_and_heading_that_exist() {
    let mut broken = Vec::new();
    for file in checked_files() {
        let text = fs::read_to_string(&file).unwrap();
        for target in links(&text) {
            if let Some(Err(why)) = resolve(&file, &target) {
                broken.push(format!(
                    "{}: {why}",
                    relative(&file.canonicalize().unwrap())
                ));
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}

#[test]
fn the_home_page_links_every_other_page_in_docs() {
    let home = root().join("docs/Home.md");
    let text = fs::read_to_string(&home).expect("docs/Home.md");
    let linked: BTreeSet<PathBuf> = links(&text)
        .iter()
        .filter_map(|t| resolve(&home, t)?.ok())
        .collect();
    let missing: Vec<String> = docs_pages()
        .into_iter()
        .filter(|p| p != &home)
        .map(|p| p.canonicalize().unwrap())
        .filter(|p| !linked.contains(p))
        .map(|p| relative(&p))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/Home.md does not link:\n{}",
        missing.join("\n")
    );
}

#[test]
fn the_readme_links_the_documentation_index() {
    let readme = root().join("README.md");
    let text = fs::read_to_string(&readme).unwrap();
    let home = root().join("docs/Home.md").canonicalize().unwrap();
    assert!(
        links(&text)
            .iter()
            .filter_map(|t| resolve(&readme, t)?.ok())
            .any(|p| p == home),
        "README.md does not link docs/Home.md"
    );
}

#[test]
fn links_ignore_code_and_read_titles() {
    let text = "See [a](one.md) and `[no](code.md)`.\n\
                ```\n[no](fenced.md)\n```\n\
                ![img](two.png \"title\") [b](three.md#x)\n";
    assert_eq!(links(text), ["one.md", "two.png", "three.md#x"]);
}

#[test]
fn anchors_follow_github_and_number_repeated_headings() {
    let text = "# Title\n\
                ## Filtering `list` and `log`\n\
                ## Next to `claude --resume`\n\
                ## Same\n\
                ## Same\n\
                ```\n## not a heading\n```\n\
                ## Diátaxis & more!\n";
    let found = anchors(text);
    for want in [
        "title",
        "filtering-list-and-log",
        "next-to-claude---resume",
        "same",
        "same-1",
        "diátaxis--more",
    ] {
        assert!(found.contains(want), "{want} missing from {found:?}");
    }
    assert!(!found.contains("not-a-heading"));
}
