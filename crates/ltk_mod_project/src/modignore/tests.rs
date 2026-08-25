use super::*;

fn temp_root(tmp: &tempfile::TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap()
}

/// Create `content/<rel>` under the root, with parent directories.
fn create_content_file(root: &Utf8Path, rel: &str) {
    let path = root.join(CONTENT_DIR_NAME).join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"x").unwrap();
}

/// Write `content/<rel_dir>/.modignore`, creating the directory.
fn write_nested_ignore(root: &Utf8Path, rel_dir: &str, text: &str) {
    let dir = root.join(CONTENT_DIR_NAME).join(rel_dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MODIGNORE_FILE_NAME), text).unwrap();
}

/// Walk `dir` and return the yielded paths relative to the content dir,
/// `/`-separated.
fn walk_files(ignore: &ModIgnore, dir: &Utf8Path) -> Vec<String> {
    ignore
        .walk(dir)
        .map(|item| {
            item.unwrap()
                .strip_prefix(ignore.content_dir())
                .unwrap()
                .as_str()
                .replace('\\', "/")
        })
        .collect()
}

/// Create a directory symlink, `false` when the platform refuses
/// (Windows without Developer Mode or admin rights).
#[cfg(windows)]
fn try_symlink_dir(target: &Utf8Path, link: &Utf8Path) -> bool {
    std::os::windows::fs::symlink_dir(target, link).is_ok()
}

#[cfg(unix)]
fn try_symlink_dir(target: &Utf8Path, link: &Utf8Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[test]
fn absent_file_yields_empty_filter() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::load(&root).unwrap();

    assert!(ignore.is_empty());
    assert_eq!(ignore.content_dir(), root.join("content"));
    assert!(!ignore.is_ignored(Utf8Path::new("base/a.psd"), false));
    assert!(matches!(
        ignore.matched(Utf8Path::new("base/a.psd"), false),
        ModIgnoreMatch::None
    ));
}

#[test]
fn empty_matches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/a.bin");

    let ignore = ModIgnore::empty(&root);

    assert!(ignore.is_empty());
    assert_eq!(
        walk_files(&ignore, ignore.content_dir()),
        vec!["base/a.bin"]
    );
}

#[test]
fn comments_and_blank_lines_are_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "# a comment\n\n\\#literal\n").unwrap();

    // `\#` escapes the comment marker into a literal file name.
    assert!(ignore.is_ignored(Utf8Path::new("base/#literal"), false));
    assert!(!ignore.is_ignored(Utf8Path::new("base/a comment"), false));
    assert!(!ignore.is_empty());
}

#[test]
fn unanchored_matches_any_depth_anchored_only_at_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "*.psd\n/base/scratch\n").unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/deep/nested/a.psd"), false));
    assert!(ignore.is_ignored(Utf8Path::new("base/scratch"), true));
    assert!(!ignore.is_ignored(Utf8Path::new("high_res/base/scratch"), true));
}

#[test]
fn directory_only_pattern_does_not_match_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "cache/\n").unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/cache"), true));
    assert!(!ignore.is_ignored(Utf8Path::new("base/cache"), false));
}

#[test]
fn negation_reincludes_and_last_match_wins() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "*.psd\n!keep.psd\n").unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/a.psd"), false));
    assert!(!ignore.is_ignored(Utf8Path::new("base/keep.psd"), false));

    let matched = ignore.matched(Utf8Path::new("base/a.psd"), false);
    let rule = matched.rule().expect("an ignore rule decided");
    assert_eq!(rule.pattern(), "*.psd");
    assert_eq!(rule.line_number(), Some(1));

    let matched = ignore.matched(Utf8Path::new("base/keep.psd"), false);
    assert!(matches!(matched, ModIgnoreMatch::Whitelist(_)));
    let rule = matched.rule().unwrap();
    assert_eq!(rule.pattern(), "!keep.psd");
    assert_eq!(rule.line_number(), Some(2));
}

#[test]
fn duplicate_patterns_report_the_deciding_line() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "*.psd\n*.psd\n").unwrap();

    let matched = ignore.matched(Utf8Path::new("base/a.psd"), false);
    assert_eq!(matched.rule().unwrap().line_number(), Some(2));
}

#[test]
fn negation_cannot_reinclude_under_an_excluded_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/ok.bin");
    create_content_file(&root, "base/scratch/keep.bin");

    let ignore = ModIgnore::parse(&root, "scratch/\n!/base/scratch/keep.bin\n").unwrap();

    // The entry alone is whitelisted; its parent decides against it.
    assert!(!ignore
        .matched(Utf8Path::new("base/scratch/keep.bin"), false)
        .is_ignored());
    assert!(ignore.is_ignored(Utf8Path::new("base/scratch/keep.bin"), false));

    // The walk never descends, so the whitelist has nothing to save.
    let mut walk = ignore.walk(ignore.content_dir());
    let files: Vec<_> = walk.by_ref().map(Result::unwrap).collect();
    assert_eq!(
        files,
        vec![root.join("content").join("base").join("ok.bin")]
    );
    assert_eq!(
        walk.skipped(),
        [root.join("content").join("base").join("scratch")]
    );
}

#[test]
fn patterns_anchor_to_content_not_project_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "/content/base/junk\n").unwrap();
    assert!(!ignore.is_ignored(Utf8Path::new("base/junk"), false));

    let ignore = ModIgnore::parse(&root, "/base/junk\n").unwrap();
    assert!(ignore.is_ignored(Utf8Path::new("base/junk"), false));
}

#[test]
fn crlf_file_parses_like_lf() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    std::fs::write(root.join(MODIGNORE_FILE_NAME), b"*.psd\r\ncache/\r\n").unwrap();

    let ignore = ModIgnore::load(&root).unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/a.psd"), false));
    assert!(ignore.is_ignored(Utf8Path::new("base/cache"), true));

    let matched = ignore.matched(Utf8Path::new("base/a.psd"), false);
    assert_eq!(matched.rule().unwrap().pattern(), "*.psd");
}

#[test]
fn bom_prefixed_file_applies_its_first_pattern() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    std::fs::write(root.join(MODIGNORE_FILE_NAME), b"\xEF\xBB\xBF*.psd\r\n").unwrap();

    let ignore = ModIgnore::load(&root).unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/a.psd"), false));
}

#[test]
fn matching_is_case_insensitive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "thumbs.db\ncache/\n/Base/Junk\n").unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/Thumbs.db"), false));
    assert!(ignore.is_ignored(Utf8Path::new("base/CACHE"), true));
    assert!(ignore.is_ignored(Utf8Path::new("base/junk"), false));
}

#[test]
fn invalid_pattern_names_the_file_and_line() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let error = ModIgnore::parse(&root, "ok\na{b\n").unwrap_err();

    match &error {
        ModIgnoreError::Pattern { path, source } => {
            assert_eq!(*path, root.join(MODIGNORE_FILE_NAME));
            assert!(
                source.to_string().contains("line 2"),
                "source must carry the line: {source}"
            );
        }
        other => panic!("expected Pattern, got {other:?}"),
    }

    // The error's own message must not repeat what its source says.
    let source = std::error::Error::source(&error).unwrap().to_string();
    assert!(
        !error.to_string().contains(&source),
        "`{error}` already contains its source `{source}`"
    );
}

#[test]
fn nested_file_anchors_at_its_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/a.psd");
    create_content_file(&root, "high_res/a.psd");
    write_nested_ignore(&root, "base", "*.psd\n/local\n");

    let ignore = ModIgnore::load(&root).unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/a.psd"), false));
    assert!(!ignore.is_ignored(Utf8Path::new("high_res/a.psd"), false));

    // `/local` anchors at `base/`, not at the content root.
    assert!(ignore.is_ignored(Utf8Path::new("base/local"), false));
    assert!(!ignore.is_ignored(Utf8Path::new("base/deep/local"), false));

    assert_eq!(
        walk_files(&ignore, ignore.content_dir()),
        vec!["high_res/a.psd"]
    );
}

#[test]
fn deeper_file_overrides_shallower() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/keep.psd");
    create_content_file(&root, "base/drop.psd");
    create_content_file(&root, "high_res/keep.psd");
    write_nested_ignore(&root, "base", "!keep.psd\n");

    // `parse` discovers nested files too, standing in for the root text.
    let ignore = ModIgnore::parse(&root, "*.psd\n").unwrap();

    assert!(!ignore.is_ignored(Utf8Path::new("base/keep.psd"), false));
    assert!(ignore.is_ignored(Utf8Path::new("base/drop.psd"), false));

    // The override holds only beneath the directory holding the file.
    assert!(ignore.is_ignored(Utf8Path::new("high_res/keep.psd"), false));
}

#[test]
fn ignore_file_under_an_ignored_directory_is_inert() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/scratch/keep.bin");
    write_nested_ignore(&root, "base/scratch", "!*\n");

    let ignore = ModIgnore::parse(&root, "scratch/\n").unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/scratch/keep.bin"), false));

    let mut walk = ignore.walk(ignore.content_dir());
    assert!(walk.by_ref().next().is_none());
    assert_eq!(
        walk.skipped(),
        [root.join("content").join("base").join("scratch")]
    );
}

#[test]
fn ignore_files_are_never_walked() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/a.bin");
    write_nested_ignore(&root, "base", "# nothing excluded\n");

    let ignore = ModIgnore::load(&root).unwrap();

    let mut walk = ignore.walk(ignore.content_dir());
    let files: Vec<_> = walk.by_ref().map(Result::unwrap).collect();
    assert_eq!(files, vec![root.join("content").join("base").join("a.bin")]);

    // Filter metadata is neither content nor "skipped by a rule".
    assert!(walk.skipped().is_empty());
}

/// Nested ignore files are found by listing the directory, so a
/// mis-cased one is interpreted on every platform instead of being
/// loaded on case-insensitive filesystems and black-holed on
/// case-sensitive ones.
#[test]
fn mis_cased_nested_ignore_file_is_still_applied() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/a.psd");
    std::fs::write(
        root.join("content").join("base").join(".MODIGNORE"),
        "*.psd\n",
    )
    .unwrap();

    let ignore = ModIgnore::load(&root).unwrap();

    assert!(ignore.is_ignored(Utf8Path::new("base/a.psd"), false));
    // And the mis-cased file itself is still not packed.
    assert_eq!(
        walk_files(&ignore, ignore.content_dir()),
        Vec::<String>::new()
    );
}

#[test]
fn invalid_nested_pattern_names_the_nested_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    write_nested_ignore(&root, "base", "a{b\n");

    let error = ModIgnore::load(&root).unwrap_err();

    match &error {
        ModIgnoreError::Pattern { path, .. } => {
            assert_eq!(
                *path,
                root.join("content").join("base").join(MODIGNORE_FILE_NAME)
            );
        }
        other => panic!("expected Pattern, got {other:?}"),
    }
}

#[test]
fn rule_attribution_names_the_source_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    write_nested_ignore(&root, "base", "# working files\n*.psd\n");

    let ignore = ModIgnore::load(&root).unwrap();

    let matched = ignore.matched(Utf8Path::new("base/a.psd"), false);
    let rule = matched.rule().unwrap();
    assert_eq!(rule.pattern(), "*.psd");
    assert_eq!(rule.line_number(), Some(2));
    assert_eq!(
        rule.source(),
        root.join("content").join("base").join(MODIGNORE_FILE_NAME)
    );
}

#[test]
fn source_files_lists_every_loaded_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    std::fs::write(root.join(MODIGNORE_FILE_NAME), "*.tmp\n").unwrap();
    write_nested_ignore(&root, "base", "*.psd\n");

    let ignore = ModIgnore::load(&root).unwrap();

    let files: Vec<Utf8PathBuf> = ignore.source_files().map(ToOwned::to_owned).collect();
    assert_eq!(
        files,
        [
            root.join(MODIGNORE_FILE_NAME),
            root.join("content").join("base").join(MODIGNORE_FILE_NAME),
        ]
    );
}

#[test]
fn walk_prunes_ignored_directories_and_sorts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/c.bin");
    create_content_file(&root, "base/a/z.bin");
    create_content_file(&root, "base/a/y.bin");
    create_content_file(&root, "base/b.bin");
    create_content_file(&root, "base/scratch/huge.psd");

    let ignore = ModIgnore::parse(&root, "scratch/\n").unwrap();
    let mut walk = ignore.walk(ignore.content_dir());
    let files: Vec<String> = walk
        .by_ref()
        .map(|item| {
            item.unwrap()
                .strip_prefix(ignore.content_dir())
                .unwrap()
                .as_str()
                .replace('\\', "/")
        })
        .collect();

    assert_eq!(
        files,
        vec!["base/a/y.bin", "base/a/z.bin", "base/b.bin", "base/c.bin"]
    );
    assert_eq!(
        walk.skipped(),
        [root.join("content").join("base").join("scratch")]
    );
}

#[test]
fn walk_of_an_ignored_directory_yields_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/X.wad.client/a.bin");

    let ignore = ModIgnore::parse(&root, "X.wad.client/\n").unwrap();
    let wad_dir = root.join("content").join("base").join("X.wad.client");

    let mut walk = ignore.walk(&wad_dir);
    assert!(walk.by_ref().next().is_none());
    assert_eq!(walk.skipped(), [wad_dir]);
}

#[test]
fn walk_of_a_subdirectory_filters_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/X.wad.client/keep.dds");
    create_content_file(&root, "base/X.wad.client/drop.psd");

    let ignore = ModIgnore::parse(&root, "*.psd\n").unwrap();
    let wad_dir = root.join("content").join("base").join("X.wad.client");

    assert_eq!(
        walk_files(&ignore, &wad_dir),
        vec!["base/X.wad.client/keep.dds"]
    );
}

#[test]
fn walk_outside_the_content_dir_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    let ignore = ModIgnore::empty(&root);

    let mut walk = ignore.walk(&root.join("elsewhere"));
    let error = walk.next().unwrap().unwrap_err();

    assert_eq!(error.path(), root.join("elsewhere"));
    assert_eq!(error.io_error().kind(), io::ErrorKind::InvalidInput);
    assert!(walk.next().is_none());
}

#[test]
fn symlinked_directory_contents_are_walked() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    let target = root.join("shared_assets");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("a.bin"), b"x").unwrap();

    let link = root.join("content").join("base").join("linked");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    if !try_symlink_dir(&target, &link) {
        eprintln!("skipping: cannot create directory symlinks here");
        return;
    }

    let ignore = ModIgnore::empty(&root);
    assert_eq!(
        walk_files(&ignore, ignore.content_dir()),
        vec!["base/linked/a.bin"]
    );
}

#[test]
fn symlink_cycle_terminates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);
    create_content_file(&root, "base/a.bin");

    let link = root.join("content").join("base").join("loop");
    if !try_symlink_dir(&root.join("content"), &link) {
        eprintln!("skipping: cannot create directory symlinks here");
        return;
    }

    let ignore = ModIgnore::empty(&root);

    // The loop is entered once and cut when it would revisit an
    // ancestor, so the walk terminates.
    let files = walk_files(&ignore, ignore.content_dir());
    assert!(files.contains(&"base/a.bin".to_string()), "{files:?}");
    assert!(files.len() < 100, "runaway walk: {} files", files.len());
}

/// In gitignore syntax a backslash escapes the next character; it is
/// NOT a path separator. A Windows author writing `base\scratch` gets a
/// pattern for a file literally named `basescratch`, which matches
/// nothing they meant. Pinned so a matcher change cannot silently alter
/// it; the README tells authors to always use `/`.
#[test]
fn backslash_in_a_pattern_is_an_escape_not_a_separator() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "base\\scratch\n").unwrap();

    // The intended directory is not matched...
    assert!(!ignore.is_ignored(Utf8Path::new("base/scratch"), true));
    // ...the escape collapses into a literal name instead.
    assert!(ignore.is_ignored(Utf8Path::new("basescratch"), false));
}

#[cfg(windows)]
#[test]
fn backslash_separated_paths_match_slash_patterns() {
    let tmp = tempfile::tempdir().unwrap();
    let root = temp_root(&tmp);

    let ignore = ModIgnore::parse(&root, "*.psd\n/base/scratch\n").unwrap();

    assert!(ignore.is_ignored(Utf8Path::new(r"base\sub\a.psd"), false));
    assert!(ignore.is_ignored(Utf8Path::new(r"base\scratch"), true));
}
