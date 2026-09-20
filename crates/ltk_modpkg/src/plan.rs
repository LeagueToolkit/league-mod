//! What unpacking a package writes, and where.
//!
//! The placement rule lives here rather than inside the extractor so that
//! asking is not the same as doing: [`ModpkgExtractor`](crate::ModpkgExtractor)
//! needs the package mutably, because writing a chunk needs the decoder, and a
//! caller that only wants to know where files would go should not have to give
//! up exclusive access to a package to find out.
//!
//! One question, one answer: [`Modpkg::extraction_plan`] is the whole plan, and
//! [`ExtractionPlan::layer`] and [`ExtractionPlan::root_files`] narrow it to the
//! parts the extractor's two halves write. The extractor writes what the plan
//! says, so the two cannot disagree about where anything goes.

use std::{
    fmt,
    io::{Read, Seek},
};

use crate::{
    chunk::ModpkgChunk, LayerIndex, Modpkg, WadIndex, HASHTABLES_CHUNK_DIR, LICENSE_CHUNK_PATH,
    README_CHUNK_PATH, THUMBNAIL_CHUNK_PATH,
};

/// The directory hashtables land under, beside the content.
///
/// A mod project keeps its tables under `hashes/` at its root; this constant
/// and that layout agree the way [`ChunkDestination::Root`]'s names agree
/// with the files a project keeps.
pub const HASHES_DIR_NAME: &str = "hashes";

/// Every chunk an unpack of a package writes, and where each one lands.
///
/// Built by [`Modpkg::extraction_plan`], and borrowed from the package it was
/// built for: the names in it are the package's own, so a plan costs one
/// allocation rather than one per chunk, and a caller sizing an unpack of tens
/// of thousands of chunks pays for none of the paths it does not build.
///
/// Ordered as an extraction walks it. A plan is a value, so it can be counted,
/// summed, narrowed and compared without being walked twice.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractionPlan<'pkg> {
    chunks: Vec<PlannedChunk<'pkg>>,
}

impl<'pkg> ExtractionPlan<'pkg> {
    /// The plan's chunks, in the order an extraction writes them.
    pub fn chunks(&self) -> &[PlannedChunk<'pkg>] {
        &self.chunks
    }

    /// The part of the plan that lands in `layer`.
    ///
    /// Empty for a layer the package holds no chunk for, including one it does
    /// not declare at all: a project may declare a layer whose content it has
    /// yet to add, and nothing is written for it either way.
    pub fn layer(&self, name: &str) -> Self {
        self.retaining(
            |destination| matches!(destination, ChunkDestination::Content { layer, .. } if *layer == name),
        )
    }

    /// The part of the plan that lands at the root: the readme, the license
    /// text and the thumbnail.
    ///
    /// Meta chunks the package stores under `_meta_/` but which do not land
    /// there: they come out under the names a mod project keeps them at,
    /// beside its content rather than inside it. Hashtables are not root
    /// files - they land under `hashes/` - so they are not here; the private
    /// `meta_files` narrowing is everything
    /// [`extract_meta`](crate::ModpkgExtractor::extract_meta) writes.
    pub fn root_files(&self) -> Self {
        self.retaining(|destination| matches!(destination, ChunkDestination::Root(_)))
    }

    /// The part of the plan that is not layer content: the root files and the
    /// hashtables.
    ///
    /// This is what [`extract_meta`](crate::ModpkgExtractor::extract_meta)
    /// writes.
    pub(crate) fn meta_files(&self) -> Self {
        self.retaining(|destination| !matches!(destination, ChunkDestination::Content { .. }))
    }

    /// The plan narrowed to the chunks whose destination `keep` accepts.
    fn retaining(&self, keep: impl Fn(&ChunkDestination<'pkg>) -> bool) -> Self {
        Self {
            chunks: self
                .chunks
                .iter()
                .copied()
                .filter(|planned| keep(&planned.destination))
                .collect(),
        }
    }
}

impl<'a, 'pkg> IntoIterator for &'a ExtractionPlan<'pkg> {
    type Item = &'a PlannedChunk<'pkg>;
    type IntoIter = std::slice::Iter<'a, PlannedChunk<'pkg>>;

    fn into_iter(self) -> Self::IntoIter {
        self.chunks.iter()
    }
}

/// One chunk of a package, and where unpacking it puts it.
///
/// The chunk rides along with its destination because a caller asking where
/// content lands is usually also asking how much of it there is:
/// [`ModpkgChunk::uncompressed_size`] sizes the unpack in bytes in the same pass
/// that sizes it in paths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannedChunk<'pkg> {
    /// The chunk to write.
    pub chunk: ModpkgChunk,
    /// Where to write it.
    pub destination: ChunkDestination<'pkg>,
}

/// Where one chunk lands when it is unpacked.
///
/// The two variants are the two roots an unpack writes into. A package
/// extracted to look at puts both under one directory - that is what
/// [`Display`](fmt::Display) spells - but a mod project keeps its content under
/// `content/` and its readme, license and thumbnail at its own root, so the
/// distinction has to survive the answer.
///
/// Not `#[non_exhaustive]`, for the reason `ltk_mod_project`'s Fantome
/// counterpart is not: a caller sizing an unpack has to account for every kind
/// of destination, and wants a compile error rather than a silent miss if one
/// is added.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChunkDestination<'pkg> {
    /// Layer content, at `{layer}/{wad}/{path}` beneath the directory the
    /// content is unpacked into.
    ///
    /// A chunk's stored path is relative to the WAD it belongs to, so the WAD
    /// name comes back as a directory or a repack cannot tell which WAD the
    /// chunk was in. A chunk belonging to no WAD has `wad: None` and sits at
    /// the layer root; a chunk several WADs share is planned once under each.
    Content {
        /// The layer the chunk belongs to.
        layer: &'pkg str,
        /// The WAD the chunk belongs to, if it belongs to one.
        wad: Option<&'pkg str>,
        /// The chunk's stored path, relative to its WAD.
        path: &'pkg str,
    },

    /// A meta chunk, written at the root of the unpack under this name:
    /// `README.md`, `LICENSE` or `thumbnail.webp`.
    ///
    /// The package stores these under `_meta_/`, but that is not where they
    /// land: the names here are the ones a mod project keeps them at, and the
    /// ones `ltk_mod_project`'s `ProjectPacker` picks back up from a project
    /// directory, so extract -> pack is a round trip.
    ///
    /// The metadata chunk is not planned at all: it is msgpack, its project-level
    /// form is `mod.config.json`, and that is not a byte-for-byte transform of
    /// it. Read it with [`Modpkg::load_metadata`] instead.
    Root(&'static str),

    /// A hashtable chunk, written at `hashes/{file_name}`.
    ///
    /// The package stores it under `_meta_/hashes/`, and `hashes/` beside
    /// the content is where a mod project keeps its tables - the same
    /// agreement by which [`Root`](Self::Root)'s names are the ones a
    /// project uses. `file_name` borrows from the package's own path table.
    ///
    /// Planned by where the chunk is stored, not by what the metadata
    /// declares: like the plan itself, this answers "where would this chunk
    /// land on disk" and never produces a table for lookup. The
    /// manifest-gated read is [`Modpkg::load_hashtables`].
    Hashtable {
        /// The table's file name, e.g. `game.hashes.txt`.
        file_name: &'pkg str,
    },
}

impl ChunkDestination<'_> {
    /// The path this chunk lands at, relative to the root it is written into.
    ///
    /// `{layer}/{wad}/{path}` for layer content and the bare file name for a
    /// root file, separated by `/` as a chunk path is. *Which* root it is
    /// relative to is the variant's business: a caller writing the two to two
    /// directories - as a mod project does - matches first and joins this onto
    /// the one it picked. A caller writing both to one, as
    /// [`ModpkgExtractor::extract_all`](crate::ModpkgExtractor::extract_all)
    /// does, joins it onto that.
    ///
    /// The same string [`Display`](fmt::Display) writes, named for what a
    /// caller does with it.
    pub fn compose(&self) -> String {
        self.to_string()
    }
}

/// Writes what [`compose`](ChunkDestination::compose) returns.
impl fmt::Display for ChunkDestination<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Content {
                layer,
                wad: Some(wad),
                path,
            } => write!(f, "{layer}/{wad}/{path}"),
            Self::Content {
                layer,
                wad: None,
                path,
            } => write!(f, "{layer}/{path}"),
            Self::Root(file_name) => f.write_str(file_name),
            Self::Hashtable { file_name } => write!(f, "{HASHES_DIR_NAME}/{file_name}"),
        }
    }
}

impl<TSource: Read + Seek> Modpkg<TSource> {
    /// What unpacking the package writes, and where.
    ///
    /// What [`ModpkgExtractor`](crate::ModpkgExtractor) writes, without writing
    /// it: the extraction writes the plan, so a caller that has to know where
    /// content will land before unpacking it - to preflight the Windows path
    /// length limit, say - cannot drift from what the unpack does.
    ///
    /// Reads the package's tables and no chunk, and takes it by reference, so
    /// asking costs neither a decompression nor exclusive access to the
    /// package.
    ///
    /// Layers are planned in name order and their WADs within them, because the
    /// header's tables are hashed and two plans for one package must not order
    /// it two different ways. The meta chunks come last.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ltk_modpkg::Modpkg;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let modpkg = Modpkg::mount_from_reader(std::fs::File::open("my-mod.modpkg")?)?;
    /// let plan = modpkg.extraction_plan();
    ///
    /// let bytes: u64 = plan
    ///     .chunks()
    ///     .iter()
    ///     .map(|planned| planned.chunk.uncompressed_size)
    ///     .sum();
    /// println!(
    ///     "unpacking writes {} files, {bytes} bytes, {} of them into the base layer",
    ///     plan.chunks().len(),
    ///     plan.layer("base").chunks().len(),
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn extraction_plan(&self) -> ExtractionPlan<'_> {
        // A chunk several WADs share has a record under each of them, so the
        // plan is built from the (wad, layer) groups rather than from the chunk
        // table: a shared chunk has to land under every WAD that claimed it.
        let mut groups: Vec<(WadIndex, LayerIndex)> =
            self.chunks_by_wad_layer.keys().copied().collect();

        // `None` is the meta group, and `Option`'s own order would put it
        // first; the readme and license belong at the end of a plan, after the
        // content they describe.
        groups.sort_by_key(|&(wad_index, layer_index)| {
            let layer = self.layer_name_for_index(layer_index);
            (layer.is_none(), layer, self.wad_name_for_index(wad_index))
        });

        let mut chunks = Vec::new();
        for (wad_index, layer_index) in groups {
            let wad = self.wad_name_for_index(wad_index);
            let layer = self.layer_name_for_index(layer_index);

            for key in self.chunks_for_wad_layer(wad_index, layer_index) {
                // Both come from tables mount fills from the same records and
                // refuses the package over, so a miss here is this crate's bug
                // rather than the package's.
                let chunk = *self
                    .chunks
                    .get(key)
                    .expect("a grouped chunk key is in the chunk table");
                let path = self
                    .chunk_path(&chunk)
                    .expect("a mounted chunk names a path table position");

                let destination = match layer {
                    Some(layer) => ChunkDestination::Content { layer, wad, path },
                    // A meta chunk with no project-level file form is left out
                    // of the plan rather than dumped under `_meta_/`.
                    None => match root_file_name(path) {
                        Some(file_name) => ChunkDestination::Root(file_name),
                        None => match hashtable_file_name(path) {
                            Some(file_name) => ChunkDestination::Hashtable { file_name },
                            None => continue,
                        },
                    },
                };

                chunks.push(PlannedChunk { chunk, destination });
            }
        }

        ExtractionPlan { chunks }
    }
}

/// The root file a meta chunk is written as, if it has one.
fn root_file_name(chunk_path: &str) -> Option<&'static str> {
    match chunk_path {
        LICENSE_CHUNK_PATH => Some("LICENSE"),
        README_CHUNK_PATH => Some("README.md"),
        THUMBNAIL_CHUNK_PATH => Some("thumbnail.webp"),
        _ => None,
    }
}

/// The file name a `_meta_/hashes/` chunk lands under, if it is one.
///
/// The placement rule behind [`ChunkDestination::Hashtable`], public so a
/// caller mapping a package's hashtable manifest to extracted files - as
/// `ltk_mod_project`'s importer does - applies the same rule the plan does
/// and the two cannot drift. `None` for a chunk path outside `_meta_/hashes/`
/// or one with no usable file name; nothing is planned for those.
///
/// The escape-proofing for hashtable destinations lives here, at the seam,
/// so every consumer of the plan inherits it: a tail that climbs, nests or
/// re-roots lands by its file name rather than steering the write, and a tail
/// with no file name plans nothing. The result is a subslice of `chunk_path`,
/// so a plan's borrows survive.
pub fn hashtable_file_name(chunk_path: &str) -> Option<&str> {
    let tail = chunk_path
        .strip_prefix(HASHTABLES_CHUNK_DIR)?
        .strip_prefix('/')?;
    let file_name = tail
        .rsplit(['/', '\\'])
        .next()
        .expect("rsplit yields at least one part");
    (!file_name.is_empty() && file_name != ".." && file_name != ".").then_some(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
        ModpkgCompression,
    };
    use std::io::Cursor;

    fn content<'a>(layer: &'a str, wad: Option<&'a str>, path: &'a str) -> ChunkDestination<'a> {
        ChunkDestination::Content { layer, wad, path }
    }

    #[test]
    fn a_chunk_lands_under_its_layer_and_its_wad() {
        assert_eq!(
            content("base", Some("Aatrox.wad.client"), "data/x.bin").compose(),
            "base/Aatrox.wad.client/data/x.bin"
        );
    }

    /// A chunk belonging to no WAD sits at the layer root, because there is no
    /// WAD directory for it to go under.
    #[test]
    fn a_chunk_without_a_wad_stays_at_the_layer_root() {
        assert_eq!(
            content("base", None, "loose.bin").compose(),
            "base/loose.bin"
        );
    }

    #[test]
    fn only_the_meta_chunks_with_a_file_form_have_a_destination() {
        assert_eq!(root_file_name(README_CHUNK_PATH), Some("README.md"));
        assert_eq!(root_file_name(LICENSE_CHUNK_PATH), Some("LICENSE"));
        assert_eq!(root_file_name(THUMBNAIL_CHUNK_PATH), Some("thumbnail.webp"));
        assert_eq!(root_file_name("_meta_/metadata"), None);
    }

    /// A plan, as the paths an unpack of it would write.
    fn paths(plan: &ExtractionPlan<'_>) -> Vec<String> {
        plan.chunks()
            .iter()
            .map(|planned| planned.destination.compose())
            .collect()
    }

    fn package(build: impl FnOnce(ModpkgBuilder) -> ModpkgBuilder) -> Modpkg<Cursor<Vec<u8>>> {
        let mut cursor = Cursor::new(Vec::new());
        build(ModpkgBuilder::default().with_layer(ModpkgLayerBuilder::base()))
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 10]))
            .unwrap();
        cursor.set_position(0);
        Modpkg::mount_from_reader(cursor).unwrap()
    }

    fn chunk(path: &str) -> ModpkgChunkBuilder {
        ModpkgChunkBuilder::new()
            .with_path(path)
            .with_compression(ModpkgCompression::None)
    }

    fn three_layers_and_a_readme() -> Modpkg<Cursor<Vec<u8>>> {
        package(|builder| {
            builder
                .with_layer(ModpkgLayerBuilder::new("zed").unwrap().with_priority(2))
                .with_layer(ModpkgLayerBuilder::new("aatrox").unwrap().with_priority(1))
                .with_readme("# My Mod\n")
                .with_chunk(chunk("x.bin").with_layer("zed"))
                .with_chunk(chunk("x.bin").with_layer("aatrox"))
                .with_chunk(chunk("x.bin"))
        })
    }

    /// The header's tables are hashed, so only a sort makes two plans for one
    /// package order it the same way.
    #[test]
    fn layers_are_planned_in_name_order_and_the_meta_chunks_last() {
        assert_eq!(
            paths(&three_layers_and_a_readme().extraction_plan()),
            ["aatrox/x.bin", "base/x.bin", "zed/x.bin", "README.md"]
        );
    }

    /// A hashtable chunk is stored under `_meta_/hashes/` but lands under
    /// `hashes/` beside the content - its own destination, not `Root`.
    #[test]
    fn a_hashtable_chunk_lands_under_the_hashes_directory() {
        let modpkg = package(|builder| {
            builder
                .with_chunk(chunk("x.bin"))
                .with_hashtable(
                    crate::ModpkgHashtable {
                        path: "_meta_/hashes/game.hashes.txt".to_string(),
                        category: ltk_hashtable::Category::Game,
                        algorithm: ltk_hashtable::Algorithm::Xxh64,
                        bits: 64,
                    },
                    "ASSETS/Custom/One.tex\n",
                )
                .unwrap()
        });

        let plan = modpkg.extraction_plan();

        assert!(paths(&plan).contains(&"hashes/game.hashes.txt".to_string()));
        // `Root` keeps its exact meaning: the readme, the license and the
        // thumbnail. A table is not a root file.
        assert!(paths(&plan.root_files()).is_empty());
    }

    /// The placement rule for `_meta_/hashes/` tails: a plain file name lands
    /// under `hashes/`, and a tail that climbs, nests or re-roots lands by
    /// its file name - so a hostile package cannot steer a write outside it.
    #[test]
    fn a_hashtable_tail_that_escapes_lands_by_its_file_name() {
        assert_eq!(
            hashtable_file_name("_meta_/hashes/game.hashes.txt"),
            Some("game.hashes.txt")
        );
        assert_eq!(
            hashtable_file_name("_meta_/hashes/../license"),
            Some("license")
        );
        assert_eq!(
            hashtable_file_name("_meta_/hashes/sub/dir.txt"),
            Some("dir.txt")
        );
        assert_eq!(
            hashtable_file_name("_meta_/hashes/evil\\name.txt"),
            Some("name.txt")
        );
        assert_eq!(hashtable_file_name("_meta_/hashes/"), None);
        assert_eq!(hashtable_file_name("_meta_/hashes/x/.."), None);
        assert_eq!(hashtable_file_name("_meta_/license"), None);
    }

    /// The two halves the extractor writes, so that narrowing and extracting
    /// cannot disagree about which chunks belong to which.
    #[test]
    fn a_plan_narrows_to_one_layer_and_to_the_root_files() {
        let modpkg = three_layers_and_a_readme();
        let plan = modpkg.extraction_plan();

        assert_eq!(paths(&plan.layer("zed")), ["zed/x.bin"]);
        assert_eq!(paths(&plan.root_files()), ["README.md"]);
    }

    /// A project may declare a layer whose content it has yet to add.
    #[test]
    fn narrowing_to_a_layer_the_package_does_not_hold_plans_nothing() {
        let modpkg = three_layers_and_a_readme();

        assert!(modpkg.extraction_plan().layer("empty").chunks().is_empty());
    }

    /// A chunk registered under several WADs is stored once and lands under
    /// every one of them, or a repack loses a WAD's membership.
    #[test]
    fn a_shared_chunk_lands_under_every_wad_that_claims_it() {
        let modpkg = package(|builder| {
            builder
                .with_chunk(chunk("data.bin").with_wad("a.wad.client"))
                .with_chunk(chunk("data.bin").with_wad("b.wad.client"))
        });

        assert_eq!(
            paths(&modpkg.extraction_plan()),
            ["base/a.wad.client/data.bin", "base/b.wad.client/data.bin"]
        );
    }

    /// The metadata chunk is msgpack; its project-level form is
    /// `mod.config.json`, which is not a copy of it, so nothing is written for
    /// it and nothing is planned for it either.
    #[test]
    fn the_metadata_chunk_is_not_planned() {
        let modpkg = package(|builder| builder.with_chunk(chunk("x.bin")));

        assert_eq!(paths(&modpkg.extraction_plan()), ["base/x.bin"]);
    }

    /// The chunk rides along with its destination, so one pass answers both
    /// how many files an unpack writes and how many bytes.
    #[test]
    fn a_plan_carries_the_chunk_it_is_for() {
        let modpkg = package(|builder| builder.with_chunk(chunk("x.bin")));
        let plan = modpkg.extraction_plan();

        let bytes: u64 = plan
            .chunks()
            .iter()
            .map(|planned| planned.chunk.uncompressed_size)
            .sum();
        assert_eq!(bytes, 10);
    }
}
