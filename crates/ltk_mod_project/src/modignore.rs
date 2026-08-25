//! Gitignore-style filtering of a mod project's `content/` directory.
//!
//! Ignore files cascade the way git's do. `<project_root>/.modignore`
//! anchors its patterns at `content/`, and any directory beneath `content/`
//! may hold its own `.modignore` whose patterns anchor at that directory.
//! A deeper file overrides shallower ones for the subtree it governs, and an
//! ignore file beneath an ignored directory is never read. The `.modignore`
//! files themselves are filter metadata, not mod content: walks never yield
//! them, so they are never packed.
//!
//! Matching follows gitignore semantics (`#` comments, `!` negation,
//! directory-only patterns, last match wins) with one deliberate deviation:
//! patterns match case-insensitively on every platform, because the game
//! resolves packed paths case-insensitively, so a case-sensitive filter
//! would let `thumbs.db` ship a `Thumbs.db`.
//!
//! # Example
//!
//! ```
//! use camino::Utf8Path;
//! use ltk_mod_project::ModIgnore;
//!
//! let root = Utf8Path::new("path/to/my-mod");
//! let ignore = ModIgnore::parse(root, "*.psd\ncache/\n!base/keep.psd\n")?;
//!
//! // Paths are relative to `content/`, or absolute beneath it.
//! assert!(ignore.is_ignored(Utf8Path::new("base/splash.psd"), false));
//! assert!(ignore.is_ignored(Utf8Path::new("base/cache"), true));
//!
//! // `!` re-includes, last match wins.
//! assert!(!ignore.is_ignored(Utf8Path::new("base/keep.psd"), false));
//! # Ok::<(), ltk_mod_project::ModIgnoreError>(())
//! ```
//!
//! The packers consume this via [`walk`](ModIgnore::walk); see its example
//! for enumerating the files that survive the filter.

use crate::CONTENT_DIR_NAME;
use camino::{Utf8Path, Utf8PathBuf};
use ignore::gitignore::{Gitignore, GitignoreBuilder, Glob};
use ignore::Match;
use std::io;
use std::rc::Rc;

/// The ignore files' name: at the project root and in any directory under
/// `content/`.
pub const MODIGNORE_FILE_NAME: &str = ".modignore";

/// Gitignore-style filter over a mod project's `content/` directory.
///
/// Built by [`load`](Self::load) from `<project_root>/.modignore` plus every
/// `.modignore` found inside `content/`, or by [`parse`](Self::parse) with
/// the root file's text supplied directly. Matching is case-insensitive on
/// every platform: the game resolves packed paths case-insensitively, so a
/// pattern that visibly names a file must not miss it over casing.
///
/// The filter and any path it is asked about must agree on the spelling of
/// the project root (both absolute or both relative), because paths are
/// related to [`content_dir`](Self::content_dir) by prefix stripping. The
/// walker returned by [`walk`](Self::walk) guarantees this by construction.
///
/// A `ModIgnore` is a point-in-time snapshot: ignore files created or
/// edited after construction are not seen. Reload before each build.
#[derive(Debug, Clone)]
pub struct ModIgnore {
    content_dir: Utf8PathBuf,
    /// Discovery order: parents before children. Matching consults the list
    /// in reverse, so deeper files override shallower ones.
    files: Vec<IgnoreFile>,
}

/// One loaded `.modignore` file.
#[derive(Debug, Clone)]
struct IgnoreFile {
    /// The file itself, for diagnostics and fingerprinting.
    path: Utf8PathBuf,
    /// Anchor directory relative to the content dir; empty for the root
    /// project file, whose patterns anchor at the content dir itself.
    rel_dir: Utf8PathBuf,
    gitignore: Gitignore,
    /// Source lines, for mapping a matched rule back to its line number.
    lines: Vec<String>,
}

impl ModIgnore {
    /// A filter that matches nothing, anchored at `<project_root>/content`.
    ///
    /// No ignore files are read, not even nested ones: this is the "filter
    /// disabled" filter.
    pub fn empty(project_root: &Utf8Path) -> Self {
        Self {
            content_dir: content_dir(project_root),
            files: Vec::new(),
        }
    }

    /// Load `<project_root>/.modignore` and every `.modignore` inside
    /// `content/`.
    ///
    /// All the files are optional: absence yields an [`empty`](Self::empty)
    /// filter. Only an unreadable file or an invalid pattern is an error;
    /// silently dropping a broken pattern would ship files the author
    /// believed were excluded. Nested files beneath an ignored directory are
    /// not read, matching git.
    ///
    /// # Example
    ///
    /// ```
    /// use camino::Utf8Path;
    /// use ltk_mod_project::{ModIgnore, MODIGNORE_FILE_NAME};
    /// # let tmp = tempfile::tempdir()?;
    /// # let project_root =
    /// #     camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    /// # std::fs::create_dir_all(project_root.join("content").join("base"))?;
    /// std::fs::write(project_root.join(MODIGNORE_FILE_NAME), "*.psd\n")?;
    /// std::fs::write(
    ///     project_root.join("content").join("base").join(MODIGNORE_FILE_NAME),
    ///     "!keep.psd\n",
    /// )?;
    ///
    /// let ignore = ModIgnore::load(&project_root)?;
    /// assert!(ignore.is_ignored(Utf8Path::new("base/splash.psd"), false));
    ///
    /// // The nested file overrides the root one for its own subtree.
    /// assert!(!ignore.is_ignored(Utf8Path::new("base/keep.psd"), false));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn load(project_root: &Utf8Path) -> Result<Self, ModIgnoreError> {
        let mut ignore = Self::empty(project_root);

        let root_path = project_root.join(MODIGNORE_FILE_NAME);
        if let Some(text) = read_ignore_file(&root_path)? {
            ignore.add_file(root_path, Utf8PathBuf::new(), &text)?;
        }

        ignore.discover_nested()?;

        Ok(ignore)
    }

    /// [`load`](Self::load), with the root file's text supplied directly,
    /// for editors and tests.
    ///
    /// `text` stands in for `<project_root>/.modignore` (which is not read,
    /// and which errors name); nested `.modignore` files inside `content/`
    /// are still discovered from disk, so parsing perfectly valid text can
    /// fail because of a broken nested file.
    pub fn parse(project_root: &Utf8Path, text: &str) -> Result<Self, ModIgnoreError> {
        let mut ignore = Self::empty(project_root);

        let root_path = project_root.join(MODIGNORE_FILE_NAME);
        ignore.add_file(root_path, Utf8PathBuf::new(), text)?;

        ignore.discover_nested()?;

        Ok(ignore)
    }

    /// Compile one ignore file and append it to the matcher list.
    fn add_file(
        &mut self,
        path: Utf8PathBuf,
        rel_dir: Utf8PathBuf,
        text: &str,
    ) -> Result<(), ModIgnoreError> {
        // Editors (notably Notepad) may prepend a UTF-8 BOM; left in place
        // it welds onto the first pattern, which then matches nothing.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);

        let anchor = self.content_dir.join(&rel_dir);
        let mut builder = GitignoreBuilder::new(anchor.as_std_path());

        // The Result in the signature is vestigial; this cannot fail.
        let _ = builder.case_insensitive(true);

        let mut lines = Vec::new();

        // `str::lines` strips `\r\n`, so Windows-authored files parse the
        // same as Unix ones; pinned by a test rather than left to chance.
        for (index, line) in text.lines().enumerate() {
            builder
                .add_line(None, line)
                .map_err(|source| ModIgnoreError::Pattern {
                    path: path.clone(),
                    source: Box::new(ignore::Error::WithLineNumber {
                        line: index as u64 + 1,
                        err: Box::new(source),
                    }),
                })?;
            lines.push(line.to_owned());
        }

        let gitignore = builder.build().map_err(|source| ModIgnoreError::Pattern {
            path: path.clone(),
            source: Box::new(source),
        })?;

        self.files.push(IgnoreFile {
            path,
            rel_dir,
            gitignore,
            lines,
        });

        Ok(())
    }

    /// Find and compile `.modignore` files inside the content directory.
    ///
    /// Pre-order, so parents land in `files` before their children, and
    /// ignored directories are pruned, so an ignore file beneath one is
    /// never read. Unreadable directories are skipped here: the walk reports
    /// them authoritatively when the content is actually enumerated.
    fn discover_nested(&mut self) -> Result<(), ModIgnoreError> {
        let mut stack: Vec<(Utf8PathBuf, Option<Rc<DirChain>>, bool)> =
            vec![(self.content_dir.clone(), None, false)];

        while let Some((dir, parent, via_link)) = stack.pop() {
            let Some(chain) = DirChain::descend(dir, parent, via_link) else {
                continue;
            };
            let dir = &chain.path;

            let Ok(entries) = dir.read_dir_utf8() else {
                continue;
            };

            let mut ignore_file: Option<Utf8PathBuf> = None;
            let mut children: Vec<(Utf8PathBuf, bool)> = Vec::new();
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                let path = entry.into_path();
                if is_ignore_file_name(&path) {
                    // Matched by name case-insensitively, read by the name
                    // the entry actually has, so a mis-cased file behaves
                    // the same on case-sensitive and case-insensitive
                    // filesystems. The exact spelling wins if several
                    // casings coexist.
                    if matches!(resolve_kind(file_type, &path), Some((false, _))) {
                        let replace = match &ignore_file {
                            None => true,
                            Some(current) => {
                                let exact = path.file_name() == Some(MODIGNORE_FILE_NAME);
                                let current_exact =
                                    current.file_name() == Some(MODIGNORE_FILE_NAME);
                                exact && !current_exact
                            }
                        };
                        if replace {
                            ignore_file = Some(path);
                        }
                    }
                    continue;
                }
                if let Some((true, is_link)) = resolve_kind(file_type, &path) {
                    children.push((path, is_link));
                }
            }

            if let Some(path) = ignore_file {
                if let Some(text) = read_ignore_file(&path)? {
                    let rel_dir = dir
                        .strip_prefix(&self.content_dir)
                        .map(ToOwned::to_owned)
                        .unwrap_or_default();
                    self.add_file(path, rel_dir, &text)?;
                }
            }

            children.sort();
            for (child, is_link) in children.into_iter().rev() {
                if !self.matched(&child, true).is_ignored() {
                    stack.push((child, Some(chain.clone()), is_link));
                }
            }
        }

        Ok(())
    }

    /// `<project_root>/content`, the root all root-file patterns anchor to.
    pub fn content_dir(&self) -> &Utf8Path {
        &self.content_dir
    }

    /// Whether the filter has no rules, and so matches nothing.
    pub fn is_empty(&self) -> bool {
        self.files.iter().all(|file| file.gitignore.is_empty())
    }

    /// The `.modignore` files the filter was built from, in discovery order
    /// (the project-root file first).
    ///
    /// Lets a caller fold them into a cache key, so an edit to any of them
    /// invalidates whatever the filter's output fed.
    pub fn source_files(&self) -> impl Iterator<Item = &Utf8Path> {
        self.files.iter().map(|file| file.path.as_path())
    }

    /// Decision for one entry, consulting the entry alone.
    ///
    /// `path` is absolute under [`content_dir`](Self::content_dir), or
    /// relative to it. Parent directories are NOT consulted: correct only
    /// when called from a walk that prunes ignored directories, where an
    /// entry under an ignored parent is never reached. For a standalone path
    /// use [`matched_with_parents`](Self::matched_with_parents).
    pub fn matched(&self, path: &Utf8Path, is_dir: bool) -> ModIgnoreMatch<'_> {
        match self.relativize(path) {
            Some(rel) => self.matched_rel(rel, is_dir),
            None => ModIgnoreMatch::None,
        }
    }

    /// Decision for one entry, also consulting every parent up to
    /// [`content_dir`](Self::content_dir).
    ///
    /// Use for paths that did not come from a pruning walk. Parents are
    /// checked top-down and the first excluded one decides, whatever deeper
    /// rules say: git's rule that a negation cannot re-include a file under
    /// an excluded directory, and exactly what a pruning walk does by never
    /// descending.
    ///
    /// # Example
    ///
    /// ```
    /// use camino::Utf8Path;
    /// use ltk_mod_project::ModIgnore;
    ///
    /// let root = Utf8Path::new("path/to/my-mod");
    /// let ignore = ModIgnore::parse(root, "scratch/\n!/base/scratch/keep.bin\n")?;
    ///
    /// // The file's own last match is the whitelist rule, but the excluded
    /// // `scratch` directory above it decides.
    /// let path = Utf8Path::new("base/scratch/keep.bin");
    /// assert!(!ignore.matched(path, false).is_ignored());
    /// assert!(ignore.matched_with_parents(path, false).is_ignored());
    /// # Ok::<(), ltk_mod_project::ModIgnoreError>(())
    /// ```
    pub fn matched_with_parents(&self, path: &Utf8Path, is_dir: bool) -> ModIgnoreMatch<'_> {
        let Some(rel) = self.relativize(path) else {
            return ModIgnoreMatch::None;
        };

        let ancestors: Vec<&Utf8Path> = rel.ancestors().collect();
        for ancestor in ancestors.into_iter().rev() {
            if ancestor.as_str().is_empty() || ancestor == rel {
                continue;
            }
            if let matched @ ModIgnoreMatch::Ignore(_) = self.matched_rel(ancestor, true) {
                return matched;
            }
        }

        self.matched_rel(rel, is_dir)
    }

    /// `matched_with_parents(..).is_ignored()`.
    pub fn is_ignored(&self, path: &Utf8Path, is_dir: bool) -> bool {
        self.matched_with_parents(path, is_dir).is_ignored()
    }

    /// Decision for a content-relative path, parents not consulted.
    ///
    /// Files are consulted deepest-first (reverse discovery order): along
    /// any directory chain the nearest `.modignore` with an opinion decides,
    /// and shallower files only fill in where deeper ones are silent. Git's
    /// precedence for cascading ignore files.
    fn matched_rel(&self, rel: &Utf8Path, is_dir: bool) -> ModIgnoreMatch<'_> {
        for file in self.files.iter().rev() {
            let Ok(in_anchor) = rel.strip_prefix(&file.rel_dir) else {
                continue;
            };
            if in_anchor.as_str().is_empty() {
                // A file never matches its own anchor directory.
                continue;
            }
            match file.gitignore.matched(in_anchor.as_std_path(), is_dir) {
                Match::None => continue,
                Match::Ignore(glob) => {
                    return ModIgnoreMatch::Ignore(ModIgnoreRule { glob, file });
                }
                Match::Whitelist(glob) => {
                    return ModIgnoreMatch::Whitelist(ModIgnoreRule { glob, file });
                }
            }
        }

        ModIgnoreMatch::None
    }

    /// Walk the files under `dir`, skipping ignored files and never
    /// descending into ignored directories.
    ///
    /// `dir` must be [`content_dir`](Self::content_dir) or a directory
    /// beneath it, spelled with the same prefix; anything else yields an
    /// error rather than silently matching nothing. A `dir` that is itself
    /// ignored yields no files and is recorded as skipped.
    ///
    /// # Example
    ///
    /// ```
    /// use ltk_mod_project::ModIgnore;
    /// # let tmp = tempfile::tempdir()?;
    /// # let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    /// # for rel in ["model.bin", "model.psd"] {
    /// #     let path = root.join("content").join("base").join(rel);
    /// #     std::fs::create_dir_all(path.parent().unwrap())?;
    /// #     std::fs::write(&path, b"x")?;
    /// # }
    /// let ignore = ModIgnore::parse(&root, "*.psd\n")?;
    ///
    /// let mut walk = ignore.walk(ignore.content_dir());
    /// let files: Vec<_> = walk.by_ref().collect::<Result<_, _>>()?;
    ///
    /// let base = ignore.content_dir().join("base");
    /// assert_eq!(files, [base.join("model.bin")]);
    /// assert_eq!(walk.skipped(), [base.join("model.psd")]);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn walk(&self, dir: &Utf8Path) -> ContentWalk<'_> {
        let mut walk = ContentWalk {
            ignore: self,
            stack: Vec::new(),
            skipped: Vec::new(),
        };

        if !dir.starts_with(&self.content_dir) {
            walk.stack.push(WorkItem::Error(ContentWalkError {
                path: dir.to_owned(),
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("not inside {}", self.content_dir),
                ),
            }));
        } else if dir != self.content_dir && self.matched_with_parents(dir, true).is_ignored() {
            walk.skipped.push(dir.to_owned());
        } else {
            walk.stack.push(WorkItem::Dir(dir.to_owned(), None, false));
        }

        walk
    }

    /// Relate `path` to `content_dir`, the form the matchers expect.
    ///
    /// An absolute path outside `content_dir` is out of scope and matches
    /// nothing; a relative path that does not start with `content_dir` is
    /// taken as already relative to it.
    fn relativize<'p>(&self, path: &'p Utf8Path) -> Option<&'p Utf8Path> {
        if let Ok(rel) = path.strip_prefix(&self.content_dir) {
            Some(rel)
        } else if path.is_relative() {
            Some(path)
        } else {
            None
        }
    }
}

fn content_dir(project_root: &Utf8Path) -> Utf8PathBuf {
    project_root.join(CONTENT_DIR_NAME)
}

/// Read an ignore file's text, `None` when the file does not exist.
fn read_ignore_file(path: &Utf8Path) -> Result<Option<String>, ModIgnoreError> {
    match std::fs::read_to_string(path.as_std_path()) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ModIgnoreError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

/// Whether the entry is named `.modignore`. Compared case-insensitively, so
/// a mis-cased file on a case-insensitive filesystem is still treated as
/// filter metadata rather than packed as content.
fn is_ignore_file_name(path: &Utf8Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(MODIGNORE_FILE_NAME))
}

/// Entry kind with symlinks resolved: `Some((is_dir, via_link))`, or `None`
/// for broken links and special files, which cannot be packed.
fn resolve_kind(file_type: std::fs::FileType, path: &Utf8Path) -> Option<(bool, bool)> {
    if file_type.is_dir() {
        Some((true, false))
    } else if file_type.is_file() {
        Some((false, false))
    } else if path.is_file() {
        Some((false, true))
    } else if path.is_dir() {
        Some((true, true))
    } else {
        None
    }
}

/// Ancestor chain of the directories along one descent path, for cutting
/// symlink and junction cycles.
///
/// Canonical forms are computed lazily, and only consulted when a link is
/// actually followed: without one, a directory can never reappear on its
/// own ancestor chain, so the common walk pays no canonicalize syscalls.
#[derive(Debug)]
struct DirChain {
    /// The directory as spelled in the walk.
    path: Utf8PathBuf,
    /// Canonical form, filled in the first time a cycle check needs it.
    real: std::cell::OnceCell<Option<std::path::PathBuf>>,
    parent: Option<Rc<DirChain>>,
}

impl DirChain {
    /// Append `dir` to the ancestor chain, or `None` when entering it would
    /// loop: it was reached through a link and its canonical form already
    /// sits on the chain.
    fn descend(
        dir: Utf8PathBuf,
        parent: Option<Rc<DirChain>>,
        via_link: bool,
    ) -> Option<Rc<DirChain>> {
        let real = std::cell::OnceCell::new();

        if via_link {
            // A canonicalize failure leaves the check off; read_dir surfaces
            // anything genuinely wrong with the directory.
            let resolved = dir.as_std_path().canonicalize().ok();
            if let Some(resolved) = &resolved {
                let mut ancestor = parent.as_ref();
                while let Some(node) = ancestor {
                    if node.real() == Some(resolved.as_path()) {
                        return None;
                    }
                    ancestor = node.parent.as_ref();
                }
            }
            let _ = real.set(resolved);
        }

        Some(Rc::new(DirChain {
            path: dir,
            real,
            parent,
        }))
    }

    fn real(&self) -> Option<&std::path::Path> {
        self.real
            .get_or_init(|| self.path.as_std_path().canonicalize().ok())
            .as_deref()
    }
}

/// Which rule, if any, decided an entry.
#[derive(Debug)]
pub enum ModIgnoreMatch<'a> {
    /// No rule matched; the entry is packed.
    None,
    /// An ignore rule matched; the entry is skipped.
    Ignore(ModIgnoreRule<'a>),
    /// A `!` rule re-included the entry; it is packed.
    Whitelist(ModIgnoreRule<'a>),
}

impl<'a> ModIgnoreMatch<'a> {
    /// Whether the entry is excluded.
    pub fn is_ignored(&self) -> bool {
        matches!(self, ModIgnoreMatch::Ignore(_))
    }

    /// The rule that decided the entry, if any did.
    pub fn rule(&self) -> Option<&ModIgnoreRule<'a>> {
        match self {
            ModIgnoreMatch::None => None,
            ModIgnoreMatch::Ignore(rule) | ModIgnoreMatch::Whitelist(rule) => Some(rule),
        }
    }
}

/// The `.modignore` line responsible for a match, for diagnostics.
///
/// # Example
///
/// ```
/// use camino::Utf8Path;
/// use ltk_mod_project::ModIgnore;
///
/// let root = Utf8Path::new("path/to/my-mod");
/// let ignore = ModIgnore::parse(root, "# working files\n*.psd\n")?;
///
/// let matched = ignore.matched(Utf8Path::new("base/splash.psd"), false);
/// let rule = matched.rule().expect("an ignore rule decided");
/// assert_eq!(rule.pattern(), "*.psd");
/// assert_eq!(rule.line_number(), Some(2));
/// assert_eq!(rule.source(), root.join(".modignore"));
/// # Ok::<(), ltk_mod_project::ModIgnoreError>(())
/// ```
#[derive(Debug)]
pub struct ModIgnoreRule<'a> {
    glob: &'a Glob,
    file: &'a IgnoreFile,
}

impl ModIgnoreRule<'_> {
    /// The pattern as written, including a `!` prefix.
    pub fn pattern(&self) -> &str {
        self.glob.original()
    }

    /// The `.modignore` file the pattern came from.
    pub fn source(&self) -> &Utf8Path {
        &self.file.path
    }

    /// The 1-based line the pattern sits on, when it can be recovered.
    ///
    /// Identical lines report the last occurrence, the one that decides
    /// under last-match-wins.
    pub fn line_number(&self) -> Option<u64> {
        let original = self.glob.original();

        self.file
            .lines
            .iter()
            .rposition(|line| trimmed(line) == original)
            .map(|index| index as u64 + 1)
    }
}

/// Trailing-whitespace trim as the parser applies it, so a source line can
/// be compared against a compiled rule's original text.
fn trimmed(line: &str) -> &str {
    if line.ends_with("\\ ") {
        line
    } else {
        line.trim_end()
    }
}

/// Failure to load a `.modignore` file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModIgnoreError {
    /// The file exists but could not be read.
    #[error("Failed to read {path}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// A pattern does not parse. This fails the pack: silently ignoring a
    /// broken pattern would ship files the author believed were excluded.
    #[error("Invalid pattern in {path}")]
    Pattern {
        path: Utf8PathBuf,
        /// Carries the offending line number, which its rendering includes.
        #[source]
        source: Box<ignore::Error>,
    },
}

/// Failure to read a directory or entry during a [`ModIgnore::walk`].
#[derive(Debug, thiserror::Error)]
#[error("Failed to read {path}")]
pub struct ContentWalkError {
    path: Utf8PathBuf,
    #[source]
    source: io::Error,
}

impl ContentWalkError {
    /// The path that could not be read.
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// The underlying IO error.
    pub fn io_error(&self) -> &io::Error {
        &self.source
    }

    /// Decompose into the path and the IO error, for callers with their own
    /// path-carrying error type.
    pub fn into_parts(self) -> (Utf8PathBuf, io::Error) {
        (self.path, self.source)
    }
}

/// Iterator over the files beneath one directory of `content/`, skipping
/// ignored files and never descending into ignored directories.
///
/// Yields files only, sorted by name within each directory, parents before
/// children, so the order is deterministic and packed archives are
/// reproducible. Symlinks are resolved: a link to a file is yielded as that
/// file, a link to a directory is descended into, and a link that loops
/// back into a directory already being descended through is cut rather than
/// followed forever. Broken links and special files are dropped without a
/// record. `.modignore` files are filter metadata and are never yielded.
///
/// Excluded entries are recorded and available from
/// [`skipped`](Self::skipped) after (or during) iteration.
#[derive(Debug)]
pub struct ContentWalk<'a> {
    ignore: &'a ModIgnore,
    stack: Vec<WorkItem>,
    skipped: Vec<Utf8PathBuf>,
}

#[derive(Debug)]
enum WorkItem {
    Dir(Utf8PathBuf, Option<Rc<DirChain>>, bool),
    File(Utf8PathBuf),
    Error(ContentWalkError),
}

impl ContentWalk<'_> {
    /// Every entry an ignore rule excluded, in traversal order.
    ///
    /// A pruned directory is recorded once; the files beneath it are not
    /// enumerated. `.modignore` files are not listed: they are metadata, not
    /// content a rule excluded.
    pub fn skipped(&self) -> &[Utf8PathBuf] {
        &self.skipped
    }

    /// Consume the walk, yielding the skipped entries.
    pub fn into_skipped(self) -> Vec<Utf8PathBuf> {
        self.skipped
    }

    fn expand(&mut self, dir: Utf8PathBuf, parent: Option<Rc<DirChain>>, via_link: bool) {
        let Some(chain) = DirChain::descend(dir, parent, via_link) else {
            // A link loops back into a directory above; descending again
            // would never end.
            return;
        };
        let dir = &chain.path;

        let entries = match dir.read_dir_utf8() {
            Ok(entries) => entries,
            Err(source) => {
                self.stack.push(WorkItem::Error(walk_error(dir, source)));
                return;
            }
        };

        let mut children: Vec<(Utf8PathBuf, bool, bool)> = Vec::new();

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(source) => {
                    self.stack.push(WorkItem::Error(walk_error(dir, source)));
                    continue;
                }
            };

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(source) => {
                    self.stack
                        .push(WorkItem::Error(walk_error(entry.path(), source)));
                    continue;
                }
            };

            let path = entry.into_path();
            if is_ignore_file_name(&path) {
                continue;
            }
            if let Some((is_dir, via_link)) = resolve_kind(file_type, &path) {
                children.push((path, is_dir, via_link));
            }
        }

        children.sort();

        let mut pending = Vec::with_capacity(children.len());
        for (path, is_dir, via_link) in children {
            if self.ignore.matched(&path, is_dir).is_ignored() {
                self.skipped.push(path);
            } else if is_dir {
                pending.push(WorkItem::Dir(path, Some(chain.clone()), via_link));
            } else {
                pending.push(WorkItem::File(path));
            }
        }

        // Reversed, so the stack pops them in sorted order.
        self.stack.extend(pending.into_iter().rev());
    }
}

impl Iterator for ContentWalk<'_> {
    type Item = Result<Utf8PathBuf, ContentWalkError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.stack.pop()? {
                WorkItem::File(path) => return Some(Ok(path)),
                WorkItem::Error(error) => return Some(Err(error)),
                WorkItem::Dir(dir, chain, via_link) => self.expand(dir, chain, via_link),
            }
        }
    }
}

fn walk_error(path: &Utf8Path, source: io::Error) -> ContentWalkError {
    ContentWalkError {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests;
