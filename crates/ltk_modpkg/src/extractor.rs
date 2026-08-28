use std::{
    fs::{self, File},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

use crate::{chunk::ModpkgChunk, error::ModpkgError, ExtractionPlan, Modpkg};

/// Extractor for ModPkg archives.
///
/// This struct provides functionality to extract chunks from a ModPkg archive
/// to a specified directory, organized by layers.
pub struct ModpkgExtractor<'modpkg, TSource: Read + Seek> {
    modpkg: &'modpkg mut Modpkg<TSource>,
}

impl<'modpkg, TSource: Read + Seek> ModpkgExtractor<'modpkg, TSource> {
    /// Create a new extractor for the given ModPkg.
    pub fn new(modpkg: &'modpkg mut Modpkg<TSource>) -> Self {
        Self { modpkg }
    }

    /// Extract all chunks from the ModPkg to the specified output directory.
    ///
    /// Content chunks are organized by layer, with each layer having its own
    /// subdirectory, in layer-name order. Meta chunks belong to no layer and are
    /// written to the output root under the names a mod project uses for them
    /// (see [`extract_meta`](Self::extract_meta)).
    ///
    /// This is a package unpacked to look at, not a mod project: a project keeps
    /// its layers under `content/` and is read from a `mod.config.json` that no
    /// chunk holds. Use `ltk_mod_project`'s `ModpkgImporter` to write one of
    /// those, or call [`extract_layer`](Self::extract_layer) and
    /// [`extract_meta`](Self::extract_meta) with the two directories yourself.
    pub fn extract_all(&mut self, output_dir: impl AsRef<Path>) -> Result<(), ModpkgError> {
        let output_dir = output_dir.as_ref();

        // Create the output directory if it doesn't exist
        fs::create_dir_all(output_dir)?;

        let steps = steps(&self.modpkg.extraction_plan(), output_dir);
        self.write_all(steps)
    }

    /// The package's layer names, in the order [`extract_all`](Self::extract_all)
    /// walks them.
    ///
    /// Sorted, because the header's layer table is hashed and two extractions
    /// of one package must not report their layers in two different orders. A
    /// layer the package declares but holds no chunk for is still listed.
    pub fn layer_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .modpkg
            .layers()
            .values()
            .map(|layer| layer.name.clone())
            .collect();
        names.sort();
        names
    }

    /// Extract one layer's chunks into `output_dir/{layer}/`.
    ///
    /// A chunk's stored path is relative to the WAD it belongs to, so it is
    /// written under a directory of that WAD's name: `{layer}/{wad}/{path}`.
    /// That is the layout a mod project holds its content in, and the one
    /// [`ModpkgBuilder`](crate::builder::ModpkgBuilder) reads WAD membership
    /// back out of, so extract -> pack keeps every chunk in the WAD it came
    /// from. A chunk belonging to no WAD is written at `{layer}/{path}`, and a
    /// chunk several WADs share is written once under each of them.
    ///
    /// A layer the package does not hold writes nothing rather than failing:
    /// a project may declare a layer whose content it has yet to add.
    pub fn extract_layer(
        &mut self,
        layer: &str,
        output_dir: impl AsRef<Path>,
    ) -> Result<(), ModpkgError> {
        let output_dir = output_dir.as_ref();

        let steps = steps(&self.modpkg.extraction_plan().layer(layer), output_dir);
        self.write_all(steps)
    }

    /// Write the meta chunks that have a project-level file form to `output_dir`.
    ///
    /// That is the readme, the license text and the thumbnail, under the names
    /// a mod project keeps them at its root, and the hashtables, under
    /// `hashes/`. Chunks without such a form (the metadata chunk, which is
    /// msgpack rather than a project file) are skipped rather than dumped
    /// under `_meta_/`; read it with [`Modpkg::load_metadata`] instead.
    pub fn extract_meta(&mut self, output_dir: impl AsRef<Path>) -> Result<(), ModpkgError> {
        let output_dir = output_dir.as_ref();

        let steps = steps(&self.modpkg.extraction_plan().meta_files(), output_dir);
        self.write_all(steps)
    }

    /// Write each chunk to the path paired with it.
    ///
    /// The paths are resolved before this is called, because writing a chunk
    /// needs the decoder, which borrows the package the plan was read from.
    fn write_all(&mut self, steps: Vec<(PathBuf, ModpkgChunk)>) -> Result<(), ModpkgError> {
        for (path, chunk) in steps {
            self.write_chunk(&chunk, &path)?;
        }

        Ok(())
    }

    /// Extract a specific chunk to the specified directory.
    pub fn extract_chunk(
        &mut self,
        chunk: &ModpkgChunk,
        output_dir: impl AsRef<Path>,
    ) -> Result<PathBuf, ModpkgError> {
        let output_dir = output_dir.as_ref();

        // Get the path for this chunk. `None` here is a chunk of some other
        // package: this is the one entry point a caller hands a record to.
        let path = match self.modpkg.chunk_path(chunk) {
            Some(path) => path,
            None => return Err(ModpkgError::MissingChunk(chunk.path_hash)),
        };

        // Create the full output path
        let output_path = output_dir.join(path);

        self.write_chunk(chunk, &output_path)?;

        Ok(output_path)
    }

    /// Decompress a chunk and write it to an exact path, creating its parents.
    fn write_chunk(&mut self, chunk: &ModpkgChunk, output_path: &Path) -> Result<(), ModpkgError> {
        // Create parent directories if they don't exist
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Load and decompress the chunk data
        let data = self.modpkg.decoder().load_chunk_decompressed(chunk)?;

        // Write the data to the output file
        let mut file = File::create(output_path)?;
        file.write_all(&data)?;

        Ok(())
    }

    /// Extract a specific chunk by its path and layer name.
    pub fn extract_chunk_by_path(
        &mut self,
        path: &str,
        layer: &str,
        output_dir: impl AsRef<Path>,
    ) -> Result<PathBuf, ModpkgError> {
        let chunk = *self.modpkg.chunk(path, Some(layer))?;
        self.extract_chunk(&chunk, output_dir)
    }
}

/// Where each of `plan`'s chunks is written, under `output_dir`.
///
/// The one place the plan becomes paths on disk, so all three extract methods
/// write where the plan says and none of them restates the layout.
fn steps(plan: &ExtractionPlan<'_>, output_dir: &Path) -> Vec<(PathBuf, ModpkgChunk)> {
    plan.chunks()
        .iter()
        .map(|planned| {
            (
                output_dir.join(planned.destination.compose()),
                planned.chunk,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
        ModpkgCompression,
    };
    use std::io::Cursor;
    use tempfile::tempdir;

    #[test]
    fn test_extractor() {
        // Create a test modpkg in memory
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let test_data = [0xAA; 100];
        let path = "test.bin";
        let layer_name = "base";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .with_compression(ModpkgCompression::None),
            );

        builder
            .build_to_writer(&mut cursor, |_| Ok(test_data.to_vec()))
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Create a temporary directory for extraction
        let temp_dir = tempdir().unwrap();
        let output_dir = temp_dir.path();

        // Create an extractor and extract all chunks
        let mut extractor = ModpkgExtractor::new(&mut modpkg);
        extractor.extract_all(output_dir).unwrap();

        // Verify the extracted file
        let extracted_file = output_dir.join(layer_name).join(path);
        assert!(extracted_file.exists());

        // Read the extracted file and verify its contents
        let extracted_data = fs::read(extracted_file).unwrap();
        assert_eq!(extracted_data, test_data);
    }

    #[test]
    fn test_extract_meta_chunks() {
        let mut cursor = Cursor::new(Vec::new());

        let license_text = "MIT License\n\nCopyright (c) 2026 Someone\n";
        let readme = "# My Mod\n";
        let thumbnail = vec![0xCC; 64];

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_readme(readme)
            .with_license_text(license_text)
            .with_thumbnail(thumbnail.clone())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.bin")
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 100]))
            .expect("Failed to build Modpkg");

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        let output_dir = temp_dir.path();

        ModpkgExtractor::new(&mut modpkg)
            .extract_all(output_dir)
            .unwrap();

        assert_eq!(
            fs::read(output_dir.join("LICENSE")).unwrap(),
            license_text.as_bytes()
        );
        assert_eq!(
            fs::read(output_dir.join("README.md")).unwrap(),
            readme.as_bytes()
        );
        assert_eq!(
            fs::read(output_dir.join("thumbnail.webp")).unwrap(),
            thumbnail
        );

        // The layer content still lands in its layer directory, and the
        // metadata chunk is not dumped as a file.
        assert!(output_dir.join("base").join("test.bin").exists());
        assert!(!output_dir.join("_meta_").exists());
    }

    /// A mod project keeps its content under `content/` and its readme, license
    /// and thumbnail at the root, so the two halves have to be writable to two
    /// directories.
    #[test]
    fn content_and_meta_extract_to_separate_directories() {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_readme(
                "# My Mod
",
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.bin")
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 100]))
            .unwrap();

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        let content = root.join("content");

        let mut extractor = ModpkgExtractor::new(&mut modpkg);
        extractor.extract_layer("base", &content).unwrap();
        extractor.extract_meta(root).unwrap();

        assert!(content.join("base").join("test.bin").exists());
        assert_eq!(
            fs::read(root.join("README.md")).unwrap(),
            b"# My Mod
"
            .to_vec()
        );
        assert!(
            !content.join("README.md").exists(),
            "meta files must not leak into the content directory"
        );
    }

    /// A hashtable is a meta chunk with a project-level file form: it lands
    /// under `hashes/` beside the content, from the same call that writes the
    /// root files, for both the look-at unpack and the project import.
    #[test]
    fn extract_meta_writes_hashtables_under_the_hashes_directory() {
        let mut cursor = Cursor::new(Vec::new());
        let names = "ASSETS/Custom/One.tex\n";

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_readme("# My Mod\n")
            .with_hashtable(
                crate::ModpkgHashtable {
                    path: "_meta_/hashes/game.hashes.txt".to_string(),
                    category: ltk_hashtable::Category::Game,
                    algorithm: ltk_hashtable::Algorithm::Xxh64,
                    bits: 64,
                },
                names,
            )
            .unwrap()
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.bin")
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 10]))
            .unwrap();

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        let root = temp_dir.path();
        ModpkgExtractor::new(&mut modpkg)
            .extract_meta(root)
            .unwrap();

        assert_eq!(
            fs::read(root.join("hashes").join("game.hashes.txt")).unwrap(),
            names.as_bytes()
        );
        assert_eq!(fs::read(root.join("README.md")).unwrap(), b"# My Mod\n");
        assert!(
            !root.join("base").exists(),
            "extract_meta must not write layer content"
        );
    }

    /// A chunk's stored path is relative to its WAD, so the WAD name has to come
    /// back as a directory or a repack cannot tell which WAD the chunk belonged
    /// to.
    #[test]
    fn wad_content_extracts_under_a_directory_of_the_wad_name() {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("data.bin")
                    .with_wad("Test.wad.client")
                    .with_compression(ModpkgCompression::None),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("loose.bin")
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 10]))
            .unwrap();

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        ModpkgExtractor::new(&mut modpkg)
            .extract_layer("base", temp_dir.path())
            .unwrap();

        let base = temp_dir.path().join("base");
        // `with_wad` lowercases the name it is given, so the directory is the
        // lowercased form. Naming the cased form here passes on a
        // case-insensitive filesystem and fails on Linux.
        assert!(base.join("test.wad.client").join("data.bin").exists());
        assert!(
            base.join("loose.bin").exists(),
            "a chunk belonging to no WAD stays at the layer root"
        );
    }

    /// The header's layer table is hashed, so only a sort makes two extractions
    /// of one package walk its layers in the same order.
    #[test]
    fn layer_names_are_sorted() {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_layer(ModpkgLayerBuilder::new("zed").unwrap().with_priority(2))
            .with_layer(ModpkgLayerBuilder::new("aatrox").unwrap().with_priority(1))
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.bin")
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 10]))
            .unwrap();

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        assert_eq!(
            ModpkgExtractor::new(&mut modpkg).layer_names(),
            ["aatrox", "base", "zed"]
        );
    }

    /// A chunk whose name is the hex of its path hash carries that hash as its
    /// `path_hash`, while the path table is keyed by the hash of the name as
    /// written. Only the chunk's table position finds it, so resolving by hash
    /// failed the whole extraction.
    #[test]
    fn a_hex_named_chunk_extracts_under_the_name_it_was_packed_with() {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_hashed_chunk_name("abcdef1234567890.dds")
                    .unwrap()
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 10]))
            .unwrap();

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        ModpkgExtractor::new(&mut modpkg)
            .extract_all(temp_dir.path())
            .unwrap();

        assert!(temp_dir
            .path()
            .join("base")
            .join("abcdef1234567890.dds")
            .exists());
    }

    /// A layer a project declares before it has content for it.
    #[test]
    fn extracting_a_layer_the_package_does_not_hold_writes_nothing() {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.bin")
                    .with_compression(ModpkgCompression::None),
            )
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 10]))
            .unwrap();

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let temp_dir = tempdir().unwrap();
        ModpkgExtractor::new(&mut modpkg)
            .extract_layer("empty", temp_dir.path())
            .unwrap();

        assert!(!temp_dir.path().join("empty").exists());
    }

    #[test]
    fn test_extract_multiple_layers() {
        // Create a test modpkg with multiple layers
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let base_data = [0xAA; 100];
        let custom_data = [0xBB; 100];
        let path = "test.bin";
        let base_layer = "base";
        let custom_layer = "custom";

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_layer(
                ModpkgLayerBuilder::new(custom_layer)
                    .unwrap()
                    .with_priority(1),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .with_compression(ModpkgCompression::None)
                    .with_layer(base_layer),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path(path)
                    .with_compression(ModpkgCompression::None)
                    .with_layer(custom_layer),
            );

        builder
            .build_to_writer(&mut cursor, |chunk| {
                if chunk.layer() == base_layer {
                    Ok(base_data.to_vec())
                } else {
                    Ok(custom_data.to_vec())
                }
            })
            .expect("Failed to build Modpkg");

        // Reset cursor and mount the modpkg
        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Create a temporary directory for extraction
        let temp_dir = tempdir().unwrap();
        let output_dir = temp_dir.path();

        // Create an extractor and extract all chunks
        let mut extractor = ModpkgExtractor::new(&mut modpkg);
        extractor.extract_all(output_dir).unwrap();

        // Verify the extracted files
        let base_file = output_dir.join(base_layer).join(path);
        let custom_file = output_dir.join(custom_layer).join(path);

        assert!(base_file.exists());
        assert!(custom_file.exists());

        // Read the extracted files and verify their contents
        let extracted_base_data = fs::read(base_file).unwrap();
        let extracted_custom_data = fs::read(custom_file).unwrap();

        assert_eq!(extracted_base_data, base_data);
        assert_eq!(extracted_custom_data, custom_data);
    }
}
