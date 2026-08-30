//! [`FantomeFormat`]: encodes a pack plan as a Fantome archive.
//!
//! Each `.wad.client` directory of the base layer is *built* into a WAD and
//! written as one stored archive entry. That is the shape distributed mods
//! overwhelmingly have and the one `ltk_fantome`'s reader can seek into; the
//! alternative the format also carries - one entry per file under
//! `WAD/<name>/` - leaves a reader a directory of loose files to rebuild a WAD
//! out of, and costs an archive the difference between a zstd-compressed WAD
//! and its files deflated one by one.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{FantomeHashtable, FantomeInfo, FantomeWriteError, FantomeWriter};
use ltk_file::LeagueFileKind;
use ltk_hashtable::{Category, Hashtable, HashtableSet, Key};
use ltk_wad::{
    chunk_hash_of, is_hex_chunk_path, strip_ltk_suffix, FileExt as _, WadBuilder, WadBuilderError,
    WadChunkBuilder, WadChunkCompression, WadHash,
};

use crate::{PackFormat, PackFormatReport, PackPlan, PackReporter, PlannedFile};

/// Failure to encode a pack plan as a Fantome archive.
///
/// Driver failures (scanning, `.modignore`, layout validation) are not here;
/// they surface as the shared variants of
/// [`PackError`](crate::PackError).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomePackError {
    /// A file in the project could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: io::Error,
    },

    /// The archive could not be written.
    #[error(transparent)]
    Write(#[from] FantomeWriteError),

    /// Two different declared hashtable files land on one archive name.
    ///
    /// Tables land flat under `META/hashes/` by file name (a `hashes/` tail
    /// is carried whole), so tables declared in different places can
    /// collide. Refused rather than renamed: the author can rename a file;
    /// a silently renamed table would ship under a name nobody chose.
    #[error(transparent)]
    DuplicateHashtableName(#[from] crate::DuplicateHashtableName),

    /// The thumbnail could not be read, or re-encoded as the PNG Fantome stores.
    #[error("Failed to convert the thumbnail {path}")]
    Thumbnail {
        path: Utf8PathBuf,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },

    /// Two files of one WAD directory address the same chunk.
    ///
    /// A WAD keys its chunks by the hash of their path, so the two would want
    /// one TOC entry between them and the pack would have to drop one. Refused
    /// rather than guessed: the author can rename a file, where a silently
    /// dropped one ships a mod missing content nobody was told about. Reachable
    /// through the `.ltk` suffix a lossless extraction adds to a path two
    /// chunks claimed, which hashes back to the path without it.
    #[error("{first} and {second} are the same chunk of {wad}")]
    ChunkCollision {
        /// The WAD directory holding both, in the author's spelling.
        wad: String,
        /// The file that claimed the chunk.
        first: String,
        /// The file that would have taken it.
        second: String,
    },

    /// A WAD could not be built from the files of its directory.
    #[error("Failed to build {wad}")]
    BuildWad {
        /// The WAD directory being built, in the author's spelling.
        wad: String,
        #[source]
        source: WadBuilderError,
    },

    /// The scratch file a WAD is built into could not be created or read back.
    ///
    /// Distinct from [`Read`](Self::Read), which names a file of the project:
    /// this names the WAD whose scratch file gave out, and there is no path to
    /// report because the file is one the pack made and never showed anyone.
    #[error("Failed to stage {wad} while packing it")]
    StageWad {
        /// The WAD being staged, in the author's spelling.
        wad: String,
        #[source]
        source: io::Error,
    },
}

impl FantomePackError {
    fn read(path: impl Into<Utf8PathBuf>, source: io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }

    fn stage(wad: impl Into<String>, source: io::Error) -> Self {
        Self::StageWad {
            wad: wad.into(),
            source,
        }
    }
}

/// Packs a mod project into a Fantome archive; the Fantome backend for
/// [`ProjectPacker`](crate::ProjectPacker).
///
/// Fantome stores less than a plan can carry: only the base layer is packed
/// (use [`ModProject::non_base_layers`](crate::ModProject::non_base_layers)
/// to warn about layers a pack will drop), and within it only files inside
/// `.wad.client` directories. See the [`pack` module docs](crate::pack) for
/// how formats plug into the driver.
///
/// # Example
///
/// ```no_run
/// use ltk_mod_project::fantome::FantomeFormat;
/// use ltk_mod_project::ProjectPacker;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let packer = ProjectPacker::from_dir("path/to/my-mod")?;
/// let file = std::fs::File::create("build/my-mod_1.0.0.fantome")?;
/// packer.pack(FantomeFormat::new(file))?;
/// # Ok(())
/// # }
/// ```
pub struct FantomeFormat<W> {
    writer: W,
}

impl<W: Write + Seek> FantomeFormat<W> {
    /// Create a format writing the archive to `writer`.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W> fmt::Debug for FantomeFormat<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FantomeFormat").finish_non_exhaustive()
    }
}

impl<W: Write + Seek> PackFormat for FantomeFormat<W> {
    type Error = FantomePackError;

    fn pack(
        self,
        plan: &PackPlan<'_>,
        progress: &mut PackReporter<'_>,
    ) -> Result<PackFormatReport, Self::Error> {
        let mut writer = FantomeWriter::new(self.writer);

        // Metadata first and the WADs after it, so every entry whose bytes
        // never move sits ahead of the ones a later repair grows.
        pack_metadata(&mut writer, plan)?;
        pack_base_layer(&mut writer, plan, progress)?;

        writer.finish()?;
        // Nothing to trim: a fantome WAD stores hashes, not paths, so no
        // stored name makes a table entry redundant.
        Ok(PackFormatReport::default())
    }
}

/// Build one WAD per `.wad.client` directory of the base layer and write each
/// as a single stored entry.
///
/// Files outside a WAD directory are neither written nor reported: Fantome has
/// no place for them.
fn pack_base_layer<W: Write + Seek>(
    writer: &mut FantomeWriter<W>,
    plan: &PackPlan<'_>,
    progress: &mut PackReporter<'_>,
) -> Result<(), FantomePackError> {
    // Grouped rather than streamed: a WAD is written TOC-first over a seekable
    // sink, and a zip entry stops being seekable the moment the next one
    // starts. Ordered by name, so one project packs to one archive whatever
    // order the scan walked its directories in.
    let mut wads: BTreeMap<&str, Vec<&PlannedFile>> = BTreeMap::new();
    for file in plan.base_layer().files() {
        if let Some(wad_name) = file.wad() {
            wads.entry(wad_name).or_default().push(file);
        }
    }

    for (wad_name, files) in wads {
        // Beside the project rather than in the system temp directory: a map
        // mod's WAD runs to hundreds of megabytes, and `%TEMP%` is routinely a
        // small system volume where `build/` is not.
        let mut built = build_wad(wad_name, &files, progress, plan.project_root())?;
        built
            .seek(SeekFrom::Start(0))
            .map_err(|source| FantomePackError::stage(wad_name, source))?;
        writer.write_packed_wad(wad_name, &mut built)?;
    }

    Ok(())
}

/// Build the WAD holding `files`, into a temporary file.
///
/// A temporary file rather than memory: a map mod's WAD runs to hundreds of
/// megabytes, and the archive entry it becomes is streamed out of this rather
/// than held beside it.
///
/// Each file's chunk hash is read back out of its path with
/// [`chunk_hash_of`], so a project extracted losslessly repacks to the chunks
/// it came from: a nameless chunk's bare hash parses as itself, a `.ltk` suffix
/// comes off, and anything else is hashed as the path it is.
///
/// Each chunk's codec is [`FileExt::ideal_compression`] over the type the
/// file's own first bytes identify - audio stored, everything else Zstd - which
/// is the policy `ltk_wad` holds and the overlay builder applies to the same
/// content. Named here rather than left to [`WadBuilder`]'s default, which
/// reads it off the whole chunk: see [`ideal_compression_of`].
///
/// Each file is reported to `progress` as the builder reaches it, which is
/// where the time actually goes - reporting them all up front would fill a
/// caller's bar instantly and then stall on the last name.
fn build_wad(
    wad_name: &str,
    files: &[&PlannedFile],
    progress: &mut PackReporter<'_>,
    scratch_dir: &Utf8Path,
) -> Result<File, FantomePackError> {
    let mut sources: BTreeMap<WadHash, &PlannedFile> = BTreeMap::new();
    let mut builder = WadBuilder::default();
    for file in files {
        let hash = chunk_hash_of(Utf8Path::new(file.rel_path()));
        if let Some(first) = sources.insert(hash, file) {
            return Err(FantomePackError::ChunkCollision {
                wad: wad_name.to_owned(),
                first: first.rel_path().to_owned(),
                second: file.rel_path().to_owned(),
            });
        }
        builder = builder.with_chunk(
            WadChunkBuilder::default()
                .with_hash(hash)
                .with_force_compression(ideal_compression_of(file.source())?),
        );
    }

    let building = RefCell::new(Building {
        progress,
        unreadable: None,
    });
    let mut wad = tempfile::tempfile_in(scratch_dir.as_std_path())
        .map_err(|source| FantomePackError::stage(wad_name, source))?;
    let built = builder.build_to_writer(&mut wad, |hash, chunk| {
        let file = sources[&hash];
        building.borrow_mut().progress.report_file(file.rel_path());

        let blame = |_: &io::Error| {
            building.borrow_mut().unreadable = Some(file.source().to_owned());
        };
        let mut content = File::open(file.source()).inspect_err(blame)?;
        io::copy(&mut content, chunk).inspect_err(blame)?;
        Ok(())
    });

    match (built, building.into_inner().unreadable) {
        (Ok(()), _) => Ok(wad),
        (Err(WadBuilderError::IoError(source)), Some(path)) => {
            Err(FantomePackError::read(path, source))
        }
        (Err(source), _) => Err(FantomePackError::BuildWad {
            wad: wad_name.to_owned(),
            source,
        }),
    }
}

/// What the chunk-data provider has to reach while the builder drives it.
///
/// Behind a [`RefCell`] because [`WadBuilder::build_to_writer`] takes its
/// provider as `Fn`. It calls it one chunk at a time and never re-enters it, so
/// no borrow here can still be held when the next call takes one.
struct Building<'a, 'p> {
    /// Where each file is reported, as the builder reaches it.
    progress: &'a mut PackReporter<'p>,
    /// The file that would not read, remembered as it fails.
    ///
    /// The builder reports a provider's failure as its own I/O error, which has
    /// no path in it, so the path has to be kept here to be named in the error
    /// the author sees.
    unreadable: Option<Utf8PathBuf>,
}

/// The names the packed WADs would otherwise lose, and where to declare them.
///
/// A packed WAD keys its chunks by the hash of their path and carries no paths
/// at all, so without this a pack of `assets/thing.bin` reads back as
/// `b0881a7f01fd23ad` - the author's own file names, gone from their own mod.
/// Harvesting them into a `game` table costs one small entry and makes the
/// archive self-describing: an import resolves its chunks through the tables
/// the archive declares before it consults any resolver a caller supplied.
///
/// The harvest adds and never repeats: a name the project's own declared tables
/// already resolve is left to them, so a project that declares a table covering
/// its chunks packs exactly as it did before this existed.
///
/// `None` when there is nothing left to record - every name already declared,
/// or a project whose files are all bare hashes, which came out of an
/// extraction that could not name them either.
///
/// A name the table grammar refuses - it is printable ASCII with `/`
/// separators, which every real WAD path is - is left out rather than failing
/// the pack. Such a chunk stays hex, which is exactly where it would have been
/// without any of this.
fn harvested_routes(
    plan: &PackPlan<'_>,
    taken: &HashSet<String>,
) -> Option<(FantomeHashtable, Hashtable)> {
    let (algorithm, width) = Category::Game.default_shape()?;
    let declared = HashtableSet::build(
        plan.hashtables()
            .iter()
            .map(|planned| (planned.entry().clone(), planned.table().clone())),
    );

    let mut names: BTreeSet<&str> = BTreeSet::new();
    for file in plan.base_layer().files() {
        if file.wad().is_none() {
            continue;
        }
        // The path the extraction would have resolved: the `.ltk` a collided
        // path gained comes off, and a chunk that landed under its bare hash
        // has no name to record.
        let named = strip_ltk_suffix(Utf8Path::new(file.rel_path()));
        if is_hex_chunk_path(named) {
            continue;
        }
        let Some(key) = Key::of(named.as_str(), &algorithm, width) else {
            continue;
        };
        if declared.resolve(&Category::Game, key).is_none() {
            names.insert(named.as_str());
        }
    }

    let mut table = Hashtable::default();
    for name in names {
        let _ = table.push(name);
    }
    table.names().next()?;
    table.sort();

    Some((
        FantomeHashtable {
            path: free_harvest_path(taken),
            category: Category::Game,
            algorithm,
            bits: width.bits(),
        },
        table,
    ))
}

/// The first harvested-table path `taken` does not already hold.
///
/// Deliberately not the conventional `game.hashes.txt`: that name belongs to a
/// table the author declared, and an archive where it sometimes means one and
/// sometimes the other is an archive nobody can read confidently. The spelling
/// is the one `ltk_fantome`'s own name merge uses for the same reason.
fn free_harvest_path(taken: &HashSet<String>) -> String {
    (1..)
        .map(|attempt| match attempt {
            1 => "META/hashes/game.harvested.hashes.txt".to_owned(),
            n => format!("META/hashes/game.harvested{n}.hashes.txt"),
        })
        .find(|candidate| !taken.contains(&candidate.to_ascii_lowercase()))
        .expect("the candidate sequence is unbounded")
}

/// Bytes of a file that have to be read before its type can be named.
///
/// The longest magic `ltk_file` matches on, and also the shortest run its
/// patterns are safe to be handed - see [`ideal_compression_of`].
const MAGIC_BYTES: std::ops::RangeInclusive<usize> = 4..=ltk_file::MAX_MAGIC_SIZE;

/// The codec `path`'s content should be stored under.
///
/// Named here rather than left to [`WadBuilder`]'s own default because that
/// default identifies the type from the *whole* chunk, and `ltk_file` 0.2.11
/// panics on a buffer of exactly three bytes: its JPEG pattern declares a
/// minimum length of three and then reads four. A mod may hold a three-byte
/// file, and a pack must not die on one. Reading the magic here bounds what the
/// identification ever sees, and a file too short for any magic is called
/// unknown rather than handed over - which changes no answer, since the only
/// two kinds that are not Zstd are named by magics of four and eight bytes.
///
/// # Errors
///
/// Fails when the file cannot be opened or read.
fn ideal_compression_of(path: &Utf8Path) -> Result<WadChunkCompression, FantomePackError> {
    let mut head = [0u8; *MAGIC_BYTES.end()];
    let mut file = File::open(path).map_err(|source| FantomePackError::read(path, source))?;
    let read =
        read_up_to(&mut file, &mut head).map_err(|source| FantomePackError::read(path, source))?;

    let kind = match read >= *MAGIC_BYTES.start() {
        true => LeagueFileKind::identify_from_bytes(&head[..read]),
        false => LeagueFileKind::Unknown,
    };
    Ok(kind.ideal_compression())
}

/// Fill as much of `buf` as `reader` has, and report how much that was.
///
/// [`Read::read`] may stop short of what it could deliver, so a single call is
/// not enough to tell a short file from a short read - and the difference
/// decides which magics a file is even eligible for.
fn read_up_to(reader: &mut impl io::Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

fn pack_metadata<W: Write + Seek>(
    writer: &mut FantomeWriter<W>,
    plan: &PackPlan<'_>,
) -> Result<(), FantomePackError> {
    // Where each planned table lands in the archive. Computed once, each
    // route carrying its table: what the routes declare is what
    // `info.hashtables` carries and what the entries below write, so the
    // entries and the manifest cannot disagree. Two manifest entries may
    // declare one file (one table, two shapes), which must stay one archive
    // entry.
    let routes = super::convert::fantome_routes(plan.hashtables())?;

    let mut info = FantomeInfo::from(plan.project());
    info.hashtables = routes.iter().map(|route| route.manifest.clone()).collect();

    let mut written = HashSet::new();
    for route in &routes {
        if written.insert(route.manifest.path.to_ascii_lowercase()) {
            writer.write_hashtable(&route.manifest, route.planned.table())?;
        }
    }

    // The chunk paths the packed WADs drop, kept beside them so an import can
    // give the author back the names they gave their own files. Declared after
    // the project's own tables and never in place of one - see
    // [`harvested_routes`].
    if let Some((manifest, table)) = harvested_routes(plan, &written) {
        writer.write_hashtable(&manifest, &table)?;
        info.hashtables.push(manifest);
    }

    writer.write_info(&info)?;

    if let Some(readme) = plan.readme() {
        let mut file =
            File::open(readme).map_err(|source| FantomePackError::read(readme, source))?;
        writer
            .write_readme(&mut file)
            .map_err(|error| attribute_write_error(error, readme))?;
    }

    // The entry keeps the canonical spelling of the source file's name
    // (`license.txt` becomes `META/LICENSE.txt`), which is what the importer
    // writes back out, so pack -> extract -> pack does not rename the file
    // underneath the author.
    if let Some(license) = plan.license() {
        let mut file = File::open(license.source())
            .map_err(|source| FantomePackError::read(license.source(), source))?;
        writer
            .write_license(license.canonical_name(), &mut file)
            .map_err(|error| attribute_write_error(error, license.source()))?;
    }

    if let Some(thumbnail) = plan.thumbnail() {
        let png = encode_thumbnail_png(thumbnail)?;
        writer.write_image_png(&png)?;
    }

    Ok(())
}

/// Re-encode the project's thumbnail as the PNG the format stores.
fn encode_thumbnail_png(image_path: &Utf8Path) -> Result<Vec<u8>, FantomePackError> {
    let thumbnail_error = |source: image::ImageError| FantomePackError::Thumbnail {
        path: image_path.to_owned(),
        source: Box::new(source),
    };

    let img = image::open(image_path).map_err(thumbnail_error)?;

    let mut png_buffer = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_buffer),
        image::ImageFormat::Png,
    )
    .map_err(thumbnail_error)?;

    Ok(png_buffer)
}

/// A copy failure while writing `path`'s entry is a read of that file as far
/// as the author is concerned; archive-level failures stay archive failures.
fn attribute_write_error(error: FantomeWriteError, path: &Utf8Path) -> FantomePackError {
    match error {
        FantomeWriteError::Io(source) => FantomePackError::read(path, source),
        other => FantomePackError::Write(other),
    }
}
