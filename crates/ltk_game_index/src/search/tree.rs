//! The object tree: every named object's path, split on `/`.

use ltk_hash::BinHash;
use serde::{Deserialize, Serialize};

use super::order::{natural_cmp, path_cmp};
use super::{GameSearch, NameTable};
use crate::DeclarationId;

/// The default [`ListOptions::limit`].
pub const DEFAULT_LISTING_LIMIT: usize = 2000;

/// What [`GameSearch::list`] lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir<'a> {
    /// The top of the tree.
    Root,
    /// A path with no leading or trailing slash, compared without regard to ASCII case.
    Path(&'a str),
    /// Every object the [`NameTable::Entries`] table does not name.
    Unnamed,
}

/// How a listing is cut.
///
/// Built as `ListOptions::default()` with fields assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ListOptions {
    /// Entries a listing holds at most. [`DEFAULT_LISTING_LIMIT`] by default.
    pub limit: usize,
    /// Whether a run of directories each holding one directory and no object folds into one
    /// entry. `false` by default.
    pub fold_single: bool,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LISTING_LIMIT,
            fold_single: false,
        }
    }
}

/// One object of the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ObjectNode {
    /// The object's path as the table writes it, or `0x` and eight lowercase hex digits for an
    /// unnamed object.
    pub path: String,
    /// The last segment of `path`, or the hash text.
    pub name: String,
    /// The object.
    pub object: BinHash,
    /// Every declaration of the object, in storage order.
    pub declarations: Vec<DeclarationId>,
}

/// One entry of a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TreeEntry {
    /// A path with objects beneath it.
    Dir {
        /// What [`Dir::Path`] takes to list it.
        path: String,
        /// The last segment, or the folded segments joined by `/`.
        name: String,
        /// Named objects anywhere beneath.
        objects: u32,
        /// The object at this path itself, when there is one.
        own: Option<ObjectNode>,
    },
    /// An object with nothing beneath it.
    Object(ObjectNode),
    /// The unnamed directory, which [`Dir::Unnamed`] lists. Root listings hold it.
    Unnamed {
        /// Objects the [`NameTable::Entries`] table does not name.
        objects: u32,
    },
}

/// One directory of the object tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Listing {
    /// Directories first, then objects, each in [`natural_cmp`] order of their names.
    pub entries: Vec<TreeEntry>,
    /// Entries before the limit.
    pub total: u32,
    /// Whether entries were cut at the limit.
    pub truncated: bool,
}

/// Every object of a term index, split into named and unnamed.
#[derive(Debug, Default)]
pub(super) struct Paths {
    /// `(Entries sheet entry, object)`, in [`path_cmp`] order of the lowercased path.
    named: Vec<(u32, BinHash)>,
    /// Every object the `Entries` sheet does not name, ascending.
    unnamed: Vec<BinHash>,
}

impl Paths {
    pub(super) fn cut(search: &GameSearch) -> Self {
        let sheet = search.names().sheet(NameTable::Entries);
        let mut paths = Self::default();
        for object in search.terms().objects().objects() {
            match search.lookup(NameTable::Entries, u64::from(object.0)) {
                Some(entry) => paths.named.push((entry, object)),
                None => paths.unnamed.push(object),
            }
        }
        sort_by_path(sheet, &mut paths.named);
        paths
    }
}

/// Sorts `named` by [`path_cmp`] of each entry's lowercased path, in parallel with the `rayon`
/// feature.
///
/// A text holding no NUL byte orders under `path_cmp` as it orders by bytes with each `/` read
/// as NUL. Such a sheet sorts by plain byte comparison over a copy with `/` replaced.
fn sort_by_path(sheet: &super::Sheet, named: &mut [(u32, BinHash)]) {
    let text = sheet.text().as_bytes();
    if text.contains(&0) {
        let compare = |a: &(u32, BinHash), b: &(u32, BinHash)| {
            path_cmp(sheet.entry_lower(a.0), sheet.entry_lower(b.0))
        };
        sort_unstable_by(named, compare);
        return;
    }
    let keys: Vec<u8> = text
        .iter()
        .map(|&byte| if byte == b'/' { 0 } else { byte })
        .collect();
    let key = |entry: u32| &keys[sheet.span(entry)];
    sort_unstable_by(named, |a, b| key(a.0).cmp(key(b.0)));
}

fn sort_unstable_by<T: Send>(
    items: &mut [T],
    compare: impl Fn(&T, &T) -> std::cmp::Ordering + Sync,
) {
    #[cfg(feature = "rayon")]
    {
        use rayon::slice::ParallelSliceMut;
        items.par_sort_unstable_by(compare);
    }
    #[cfg(not(feature = "rayon"))]
    items.sort_unstable_by(compare);
}

/// Whether `rest` is `segment` or opens with `segment/`.
fn shares_segment(rest: &str, segment: &str) -> bool {
    rest.strip_prefix(segment)
        .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
}

/// The segment `rest` opens with.
fn first_segment(rest: &str) -> &str {
    rest.split('/').next().unwrap_or(rest)
}

/// `path` lowercased, without leading or trailing slashes.
fn normalize(path: &str) -> String {
    path.trim_matches('/').to_ascii_lowercase()
}

/// One entry of a listing before its nodes are built.
///
/// The entry's path is `head`'s lowercased path up to `end`, and its name the part from
/// `start`.
#[derive(Debug, Clone, Copy)]
struct Pending {
    head: u32,
    start: usize,
    end: usize,
    kind: PendingKind,
}

#[derive(Debug, Clone, Copy)]
enum PendingKind {
    Dir { objects: u32, own: Option<BinHash> },
    Object(BinHash),
}

impl GameSearch {
    /// One directory of the object tree. `None` for a [`Dir::Path`] no object path runs
    /// through.
    ///
    /// A path that is an object and the parent of others is one [`TreeEntry::Dir`] with `own`
    /// set. The root lists [`TreeEntry::Unnamed`] last among directories when any object is
    /// unnamed. [`Dir::Unnamed`] lists one object per unnamed hash, ascending. Nodes are built
    /// for the entries within the limit alone.
    #[must_use]
    pub fn list(&self, dir: Dir<'_>, options: &ListOptions) -> Option<Listing> {
        let paths = self.paths();
        let prefix = match dir {
            Dir::Unnamed => {
                let total = paths.unnamed.len();
                let entries = paths
                    .unnamed
                    .iter()
                    .take(options.limit)
                    .map(|&object| TreeEntry::Object(self.unnamed_node(object)))
                    .collect();
                return Some(Listing {
                    entries,
                    total: total as u32,
                    truncated: total > options.limit,
                });
            }
            Dir::Root => String::new(),
            Dir::Path(path) => match normalize(path) {
                path if path.is_empty() => path,
                path => format!("{path}/"),
            },
        };
        let (dirs, objects) = self.pending(paths, &prefix, options);
        if !prefix.is_empty() && dirs.is_empty() && objects.is_empty() {
            return None;
        }
        let unnamed =
            (prefix.is_empty() && !paths.unnamed.is_empty()).then_some(TreeEntry::Unnamed {
                objects: paths.unnamed.len() as u32,
            });
        let total = dirs.len() + usize::from(unnamed.is_some()) + objects.len();
        let mut entries = Vec::with_capacity(total.min(options.limit));
        let mut dirs = dirs.into_iter().map(|pending| self.entry(pending));
        let mut objects = objects.into_iter().map(|pending| self.entry(pending));
        entries.extend(dirs.by_ref().take(options.limit));
        if let Some(unnamed) = unnamed
            && entries.len() < options.limit
        {
            entries.push(unnamed);
        }
        let room = options.limit - entries.len();
        entries.extend(objects.by_ref().take(room));
        Some(Listing {
            entries,
            total: total as u32,
            truncated: total > options.limit,
        })
    }

    /// Every named object at or below `prefix`, as its lowercased path and hash, in
    /// [`path_cmp`] order of the lowercased path.
    ///
    /// `prefix` is compared without regard to ASCII case. An empty prefix yields every named
    /// object.
    pub fn under(&self, prefix: &str) -> impl Iterator<Item = (&str, BinHash)> + '_ {
        let prefix = normalize(prefix);
        let sheet = self.names().sheet(NameTable::Entries);
        let named = &self.paths().named;
        let start = named
            .partition_point(|&(entry, _)| path_cmp(sheet.entry_lower(entry), &prefix).is_lt());
        named[start..]
            .iter()
            .map(move |&(entry, object)| (sheet.entry_lower(entry), object))
            .take_while(move |(path, _)| {
                prefix.is_empty()
                    || path
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
            })
    }

    /// The node at `path`, when an object has that path.
    ///
    /// `path` is compared without regard to ASCII case.
    #[must_use]
    pub fn node(&self, path: &str) -> Option<ObjectNode> {
        let path = normalize(path);
        let sheet = self.names().sheet(NameTable::Entries);
        let named = &self.paths().named;
        let at = named
            .binary_search_by(|&(entry, _)| path_cmp(sheet.entry_lower(entry), &path))
            .ok()?;
        let (entry, object) = named[at];
        let written = sheet.entry(entry);
        let name = written.rsplit('/').next().unwrap_or(&written).to_owned();
        Some(self.named_node(written.into_owned(), name, object))
    }

    /// The directories and the objects directly under `prefix`, which is empty or ends with
    /// `/`, each in [`natural_cmp`] order of their names.
    fn pending(
        &self,
        paths: &Paths,
        prefix: &str,
        options: &ListOptions,
    ) -> (Vec<Pending>, Vec<Pending>) {
        let sheet = self.names().sheet(NameTable::Entries);
        let lower = |entry: u32| sheet.entry_lower(entry);
        let start = paths
            .named
            .partition_point(|&(entry, _)| path_cmp(lower(entry), prefix).is_lt());
        let len =
            paths.named[start..].partition_point(|&(entry, _)| lower(entry).starts_with(prefix));
        let run = &paths.named[start..start + len];
        let cut = prefix.len();

        let mut dirs = Vec::new();
        let mut objects = Vec::new();
        let mut at = 0;
        while at < run.len() {
            let segment = first_segment(&lower(run[at].0)[cut..]);
            let end = at
                + run[at..]
                    .partition_point(|&(entry, _)| shares_segment(&lower(entry)[cut..], segment));
            let group = &run[at..end];
            let (head, object) = group[0];
            let path_end = cut + segment.len();
            let own = (lower(head).len() == path_end).then_some(object);
            let children = &group[usize::from(own.is_some())..];
            match (own, children.first()) {
                (Some(object), None) => objects.push(Pending {
                    head,
                    start: cut,
                    end: path_end,
                    kind: PendingKind::Object(object),
                }),
                (own, _) => {
                    let end = match options.fold_single && own.is_none() {
                        true => fold(sheet, children, path_end),
                        false => path_end,
                    };
                    dirs.push(Pending {
                        head,
                        start: cut,
                        end,
                        kind: PendingKind::Dir {
                            objects: children.len() as u32,
                            own,
                        },
                    });
                }
            }
            at = end;
        }
        let name = |pending: &Pending| &lower(pending.head)[pending.start..pending.end];
        dirs.sort_by(|a, b| natural_cmp(name(a), name(b)));
        objects.sort_by(|a, b| natural_cmp(name(a), name(b)));
        (dirs, objects)
    }

    /// The entry `pending` names, with its nodes built.
    fn entry(&self, pending: Pending) -> TreeEntry {
        let written = self.names().sheet(NameTable::Entries).entry(pending.head);
        let path = written[..pending.end].to_owned();
        let name = written[pending.start..pending.end].to_owned();
        match pending.kind {
            PendingKind::Object(object) => TreeEntry::Object(self.named_node(path, name, object)),
            PendingKind::Dir { objects, own } => {
                let own = own.map(|object| self.named_node(path.clone(), name.clone(), object));
                TreeEntry::Dir {
                    path,
                    name,
                    objects,
                    own,
                }
            }
        }
    }

    fn named_node(&self, path: String, name: String, object: BinHash) -> ObjectNode {
        ObjectNode {
            path,
            name,
            object,
            declarations: self.declarations_of(object),
        }
    }

    fn unnamed_node(&self, object: BinHash) -> ObjectNode {
        let text = format!("0x{:08x}", object.0);
        ObjectNode {
            path: text.clone(),
            name: text,
            object,
            declarations: self.declarations_of(object),
        }
    }

    fn declarations_of(&self, object: BinHash) -> Vec<DeclarationId> {
        self.terms()
            .objects()
            .declarations(object)
            .map(|(id, _)| id)
            .collect()
    }
}

/// The end of the path under `end` after folding every directory holding one directory and no
/// object.
///
/// `group` is every object under the path of `end` bytes, in path order, none of them at it.
fn fold(sheet: &super::Sheet, group: &[(u32, BinHash)], mut end: usize) -> usize {
    let Some(&(head, _)) = group.first() else {
        return end;
    };
    let lower = sheet.entry_lower(head);
    loop {
        let cut = end + 1;
        let next = first_segment(&lower[cut..]);
        let one_dir = group.iter().all(|&(entry, _)| {
            let rest = &sheet.entry_lower(entry)[cut..];
            rest.len() > next.len() && shares_segment(rest, next)
        });
        if !one_dir {
            return end;
        }
        end = cut + next.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sheet;

    #[test]
    fn the_byte_key_sort_orders_as_path_cmp_and_a_nul_falls_back_to_it() {
        for with_nul in [false, true] {
            let mut paths = vec![
                "a-b", "a/b", "a", "a/b/c", "a/c", "a.b", "A/B/d", "b", "a!", "a/b-c", "a/b/c/d",
            ];
            if with_nul {
                paths.push("a\0z");
            }
            let sheet = Sheet::build(paths.iter().enumerate().map(|(at, p)| (*p, at as u64)));
            let mut named: Vec<(u32, BinHash)> =
                (0..sheet.len() as u32).map(|e| (e, BinHash(e))).collect();
            sort_by_path(&sheet, &mut named);
            let sorted: Vec<&str> = named.iter().map(|&(e, _)| sheet.entry_lower(e)).collect();
            let mut expected: Vec<&str> = (0..sheet.len() as u32)
                .map(|e| sheet.entry_lower(e))
                .collect();
            expected.sort_by(|a, b| path_cmp(a, b));
            assert_eq!(sorted, expected, "{with_nul}");
        }
    }
}
