//! Runs `scripts/wiki.sh`, which builds the wiki pages from `docs/`, on small fixtures and on the
//! real documentation.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/wiki.sh")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sessions-wiki-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

fn build(src: &Path, wiki: &Path) -> Output {
    Command::new("sh")
        .arg(script())
        .arg(src)
        .arg(wiki)
        .arg("v1.2.3")
        .arg("owner/repo")
        .output()
        .unwrap()
}

fn read(wiki: &Path, page: &str) -> String {
    fs::read_to_string(wiki.join(page)).unwrap_or_else(|e| panic!("{page}: {e}"))
}

fn pages(wiki: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(wiki)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

/// A docs tree with every kind of link the converter has to handle.
fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let root = scratch(name);
    let (src, wiki) = (root.join("src"), root.join("wiki"));
    fs::create_dir_all(&wiki).unwrap();
    write(
        &src,
        "docs/Home.md",
        "# Home\n\
         \n\
         ## Learn\n\
         \n\
         - [Install](../README.md#install) and [more](../AGENTS.md)\n\
         - [Tool](reference/tool.md)\n\
         \n\
         ## Know\n\
         \n\
         - [Design](explanation/design.md#why \"a title\"): why\n\
         - [Decision](adr/0001-first.md)\n\
         - [Site](https://example.test/x)\n\
         - plain text without a link\n\
         \n\
         ## Only elsewhere\n\
         \n\
         - [Readme](../README.md)\n\
         \n\
         `[inline](code.md)` stays, and so does\n\
         \n\
         ```md\n\
         [fenced](code.md)\n\
         ```\n\
         \n\
         [Jump](#know) and [dir](../scripts/) and [up](../../outside.md)\n",
    );
    write(
        &src,
        "docs/reference/tool.md",
        "# Tool\n\n[Design](../explanation/design.md) [Home](../Home.md#learn) [Code](../../src/lib.rs)\n",
    );
    write(&src, "docs/explanation/design.md", "# Design\n");
    write(&src, "docs/how-to/make-a-release.md", "# Release\n");
    write(&src, "docs/adr/0001-first.md", "# ADR\n");
    (src, wiki)
}

#[test]
fn pages_get_flat_names_and_a_footer() {
    let (src, wiki) = fixture("names");
    let out = build(&src, &wiki);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        pages(&wiki),
        [
            "ADR-0001-first.md",
            "Explanation-design.md",
            "Home.md",
            "How-to-make-a-release.md",
            "Reference-tool.md",
            "_Sidebar.md"
        ]
    );
    let footer = "generated from [docs/reference/tool.md](https://github.com/owner/repo/blob/v1.2.3/docs/reference/tool.md) at v1.2.3";
    assert!(read(&wiki, "Reference-tool.md").contains(footer));
}

#[test]
fn links_between_pages_become_page_names_and_others_point_at_the_tag_on_github() {
    let (src, wiki) = fixture("links");
    assert!(build(&src, &wiki).status.success());
    let home = read(&wiki, "Home.md");
    let blob = "https://github.com/owner/repo/blob/v1.2.3";
    for want in [
        format!("[Install]({blob}/README.md#install)"),
        format!("[more]({blob}/AGENTS.md)"),
        "[Tool](Reference-tool)".to_string(),
        "[Design](Explanation-design#why \"a title\")".to_string(),
        "[Decision](ADR-0001-first)".to_string(),
        "[Site](https://example.test/x)".to_string(),
        "[Jump](#know)".to_string(),
        "[dir](https://github.com/owner/repo/tree/v1.2.3/scripts)".to_string(),
        // A link that climbs out of the repository cannot be resolved; it must not crash.
        format!("[up]({blob}/outside.md)"),
    ] {
        assert!(home.contains(&want), "missing {want}\n{home}");
    }
    let tool = read(&wiki, "Reference-tool.md");
    assert!(tool.contains("[Design](Explanation-design)"), "{tool}");
    assert!(tool.contains("[Home](Home#learn)"), "{tool}");
    assert!(
        tool.contains(&format!("[Code]({blob}/src/lib.rs)")),
        "{tool}"
    );
}

#[test]
fn code_blocks_and_inline_code_are_copied_unchanged() {
    let (src, wiki) = fixture("code");
    assert!(build(&src, &wiki).status.success());
    let home = read(&wiki, "Home.md");
    assert!(home.contains("`[inline](code.md)` stays"), "{home}");
    assert!(home.contains("```md\n[fenced](code.md)\n```"), "{home}");
}

#[test]
fn the_sidebar_links_each_section_and_lists_only_the_pages_that_are_in_the_wiki() {
    let (src, wiki) = fixture("sidebar");
    assert!(build(&src, &wiki).status.success());
    let sidebar = read(&wiki, "_Sidebar.md");
    assert_eq!(
        sidebar,
        "[Home](Home)\n\
         \n**[Learn](Home#learn)**\n\
         \n* [Tool](Reference-tool)\n\
         \n**[Know](Home#know)**\n\
         \n* [Design](Explanation-design#why)\n\
         * [Decision](ADR-0001-first)\n\
         \n**[Only elsewhere](Home#only-elsewhere)**\n",
        "links that leave the wiki and items without a link are left out, and a link title is dropped"
    );
}

#[test]
fn pages_that_left_docs_are_removed_and_other_files_are_kept() {
    let (src, wiki) = fixture("stale");
    write(&wiki, "Old-page.md", "gone from docs\n");
    write(&wiki, ".git/HEAD", "ref: refs/heads/master\n");
    write(&wiki, "image.png", "not a page");
    assert!(build(&src, &wiki).status.success());
    let found = pages(&wiki);
    assert!(!found.contains(&"Old-page.md".to_string()), "{found:?}");
    assert!(found.contains(&".git".to_string()) && found.contains(&"image.png".to_string()));
}

#[test]
fn it_stops_with_a_message_when_the_ref_has_no_home_page() {
    let root = scratch("nohome");
    let (src, wiki) = (root.join("src"), root.join("wiki"));
    fs::create_dir_all(src.join("docs")).unwrap();
    fs::create_dir_all(&wiki).unwrap();
    let out = build(&src, &wiki);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("has no docs/Home.md"));
    assert!(pages(&wiki).is_empty());
}

/// Every link in the pages built from the real `docs/` leads somewhere that exists in the wiki or
/// on GitHub at the tag.
#[test]
fn the_real_documentation_converts_without_a_dangling_link() {
    let root = scratch("real");
    let wiki = root.join("wiki");
    fs::create_dir_all(&wiki).unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(build(&repo, &wiki).status.success());

    let found = pages(&wiki);
    assert!(found.contains(&"Home.md".to_string()) && found.contains(&"_Sidebar.md".to_string()));
    let names: Vec<String> = found
        .iter()
        .map(|p| p.trim_end_matches(".md").to_string())
        .collect();
    for page in &found {
        let text = read(&wiki, page);
        let mut rest = text.as_str();
        while let Some(i) = rest.find("](") {
            rest = &rest[i + 2..];
            let target = rest[..rest.find(')').unwrap()]
                .split_whitespace()
                .next()
                .unwrap();
            let name = target.split('#').next().unwrap();
            let ok = target.starts_with('#')
                || target.starts_with("https://")
                || names.iter().any(|n| n == name);
            assert!(ok, "{page}: {target} leads nowhere");
            if !target.starts_with("https://") {
                assert!(
                    !name.ends_with(".md"),
                    "{page}: {target} is still a file link"
                );
            }
        }
    }
}
