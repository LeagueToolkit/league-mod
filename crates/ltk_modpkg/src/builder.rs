use binrw::BinWrite;
use byteorder::{WriteBytesExt, LE};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64;

use crate::{
    chunk::ModpkgChunk,
    metadata::{ModpkgMetadata, METADATA_CHUNK_PATH},
    thumbnail::THUMBNAIL_CHUNK_PATH,
    ChunkKey, LayerHash, LayerIndex, ModpkgCompression, PathHash, WadIndex, WadNameHash,
};
use crate::{ChunkPath, Slug, BASE_LAYER_NAME, LICENSE_CHUNK_PATH, README_CHUNK_PATH};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModpkgBuilderError {
    #[error("io error")]
    Io(#[from] io::Error),

    /// A chunk's binary layout could not be written.
    ///
    /// The underlying writer error is boxed so the crate used to write it is
    /// not part of this crate's public API.
    #[error("Failed to write binary layout")]
    BinWrite(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("modpkg error")]
    Modpkg(#[from] crate::error::ModpkgError),

    #[error("unsupported compression type: {0:?}")]
    UnsupportedCompressionType(ModpkgCompression),

    #[error("missing base layer")]
    MissingBaseLayer,

    #[error("layer not found: {0}")]
    LayerNotFound(String),

    #[error("invalid chunk name: {0}")]
    InvalidChunkName(String),

    #[error("invalid layer name")]
    InvalidLayerName(#[from] crate::error::InvalidSlugError),

    /// One chunk received different content for different WADs.
    ///
    /// Entries that share a `(path, layer)` identity are one chunk registered
    /// under several WADs, so they must be given byte-identical content.
    #[error(
        "inconsistent content for chunk {path} in layer {layer}: \
         WADs {first_wad} and {second_wad}"
    )]
    InconsistentChunk {
        path: String,
        layer: String,
        first_wad: String,
        second_wad: String,
    },
}

// See the matching impl on `ModpkgError`: the conversion names `binrw::Error`
// so `?` keeps working, while the variant does not.
impl From<binrw::Error> for ModpkgBuilderError {
    fn from(error: binrw::Error) -> Self {
        Self::BinWrite(Box::new(error))
    }
}

/// Provides an interface to build a Modpkg file.
///
/// Meta chunks (metadata, readme, license text, thumbnail) are not held as
/// chunk builders: they are derived from the content fields by
/// [`meta_chunks`](Self::meta_chunks) at build time.
///
/// The readme and license text are bytes, not `String`. They are copied
/// byte-for-byte from the project and never inspected, so decoding them would
/// only create the chance to mangle a file whose exact contents matter. Callers
/// that display one decode it at that point.
#[derive(Debug, Clone, Default)]
pub struct ModpkgBuilder {
    readme: Option<Vec<u8>>,
    license_text: Option<Vec<u8>>,
    thumbnail: Option<Vec<u8>>,
    metadata: ModpkgMetadata,
    // The WAD is part of the key so one chunk identity can be registered
    // under several WADs; see `with_chunk`.
    chunks: HashMap<(ChunkKey, WadNameHash), ModpkgChunkBuilder>,
    layers: Vec<ModpkgLayerBuilder>,
}

/// The identity of a chunk's data: `(uncompressed_checksum, uncompressed_size)`.
type ContentKey = (u64, u64);

/// A meta chunk to be written, resolved from the builder's content fields.
#[derive(Debug)]
struct MetaChunk<'builder> {
    path: &'static str,
    data: Cow<'builder, [u8]>,
    compression: ModpkgCompression,
}

impl MetaChunk<'_> {
    fn path_hash(&self) -> PathHash {
        ChunkPath::new(self.path).hash()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModpkgChunkBuilder {
    path_hash: PathHash,
    path: String,
    compression: ModpkgCompression,
    layer: String,
    wad: String,
}

#[derive(Debug, Clone)]
pub struct ModpkgLayerBuilder {
    name: Slug,
    priority: i32,
}

impl Default for ModpkgLayerBuilder {
    fn default() -> Self {
        Self::base()
    }
}

impl ModpkgBuilder {
    /// Add a layer to the builder.
    pub fn with_layer(mut self, layer: ModpkgLayerBuilder) -> Self {
        self.layers.push(layer);
        self
    }

    /// Add a chunk to the builder.
    ///
    /// Chunks are stored by `(path, layer, WAD)`: a chunk with all three equal
    /// to an existing one replaces it. Chunks that share a `(path, layer)`
    /// identity under different WADs are all kept - they register one chunk
    /// under several WADs, and [`build_to_writer`](Self::build_to_writer)
    /// rejects them unless their content is byte-identical.
    pub fn with_chunk(mut self, chunk: ModpkgChunkBuilder) -> Self {
        let key = chunk.full_key();
        self.chunks.insert(key, chunk);
        self
    }

    /// Build the Modpkg file and write it to the given writer.
    ///
    /// * `writer` - The writer to write the Modpkg file to.
    /// * `provide_chunk_data` - A function that returns the uncompressed data
    ///   for each chunk; it is called once per registered chunk.
    pub fn build_to_writer<
        TWriter: io::Write + io::Seek,
        TChunkDataProvider: FnMut(&ModpkgChunkBuilder) -> Result<Vec<u8>, ModpkgBuilderError>,
    >(
        self,
        writer: &mut TWriter,
        provide_chunk_data: TChunkDataProvider,
    ) -> Result<(), ModpkgBuilderError> {
        let mut writer = BufWriter::new(writer);

        // Resolve exactly what will be written before writing anything: the
        // header's chunk count and the reserved TOC are both derived from these
        // two lists, so they cannot disagree with the entries written later.
        let meta_chunks = self.meta_chunks()?;
        let meta_path_hashes: HashSet<PathHash> =
            meta_chunks.iter().map(MetaChunk::path_hash).collect();
        let regular_chunks = self.collect_regular_chunks(&meta_path_hashes);

        // Collect all unique paths, layers, and wads
        let (chunk_paths, chunk_path_indices) =
            Self::collect_unique_paths(&meta_chunks, &regular_chunks);
        let (layers, _) = Self::collect_unique_layers(&regular_chunks);
        let (wads, wad_indices) = Self::collect_unique_wads(&regular_chunks);

        Self::validate_layers(&self.layers, &layers)?;

        let total_chunks = meta_chunks.len() + regular_chunks.len();

        Self::write_header(&mut writer, total_chunks)?;
        Self::write_layers(&mut writer, &self.layers)?;
        Self::write_chunk_paths(&mut writer, &chunk_paths)?;
        Self::write_wads(&mut writer, &wads)?;
        Self::write_alignment(&mut writer)?;

        // Reserve space for chunk TOC
        let chunk_toc_offset = writer.stream_position()?;
        writer.write_all(&vec![0; total_chunks * ModpkgChunk::RECORD_SIZE])?;

        let layer_index_map = Self::build_layer_index_map(&self.layers);

        let all_chunks = Self::process_all_chunks(
            &mut writer,
            provide_chunk_data,
            &meta_chunks,
            &regular_chunks,
            &chunk_path_indices,
            &layer_index_map,
            &wad_indices,
        )?;

        // Go back and write the actual chunk TOC
        Self::write_chunk_toc(&mut writer, chunk_toc_offset, &all_chunks)?;

        Ok(())
    }

    fn write_header<W: io::Write>(
        writer: &mut W,
        total_chunks: usize,
    ) -> Result<(), ModpkgBuilderError> {
        // Write magic header
        writer.write_all(b"_modpkg_")?;

        // Write version
        writer.write_u32::<LE>(1)?;

        // Write signature size and chunk count
        writer.write_u32::<LE>(0)?; // Placeholder for signature size
        writer.write_u32::<LE>(total_chunks as u32)?;

        // Write signature (empty for now)
        let signature = Vec::new();
        writer.write_all(&signature)?;

        Ok(())
    }

    fn write_layers<W: io::Write>(
        writer: &mut W,
        layers: &[ModpkgLayerBuilder],
    ) -> Result<(), ModpkgBuilderError> {
        writer.write_u32::<LE>(layers.len() as u32)?;
        for layer in layers {
            writer.write_u32::<LE>(layer.name.as_str().len() as u32)?;
            writer.write_all(layer.name.as_str().as_bytes())?;
            writer.write_i32::<LE>(layer.priority)?;
        }
        Ok(())
    }

    fn write_chunk_paths<W: io::Write>(
        writer: &mut W,
        chunk_paths: &[String],
    ) -> Result<(), ModpkgBuilderError> {
        // Write count
        writer.write_u32::<LE>(chunk_paths.len() as u32)?;

        // Write all chunk paths (including meta chunks)
        for path in chunk_paths {
            writer.write_all(path.as_bytes())?;
            writer.write_all(&[0])?; // Null terminator
        }

        Ok(())
    }

    fn write_wads<W: io::Write>(writer: &mut W, wads: &[String]) -> Result<(), ModpkgBuilderError> {
        writer.write_u32::<LE>(wads.len() as u32)?;
        for wad in wads {
            writer.write_all(wad.as_bytes())?;
            writer.write_all(&[0])?; // Null terminator
        }
        Ok(())
    }

    fn write_alignment<W: io::Write + io::Seek>(writer: &mut W) -> Result<(), ModpkgBuilderError> {
        let current_pos = writer.stream_position()?;
        let padding = (8 - (current_pos % 8)) % 8;
        for _ in 0..padding {
            writer.write_all(&[0])?;
        }
        Ok(())
    }

    fn build_layer_index_map(layers: &[ModpkgLayerBuilder]) -> HashMap<LayerHash, LayerIndex> {
        let mut layer_index_map = HashMap::new();
        for (idx, layer) in layers.iter().enumerate() {
            layer_index_map.insert(
                LayerHash::from_name(layer.name.as_str()),
                LayerIndex::new(idx as u32),
            );
        }
        layer_index_map
    }

    fn process_all_chunks<
        TWriter: io::Write + io::Seek,
        TChunkDataProvider: FnMut(&ModpkgChunkBuilder) -> Result<Vec<u8>, ModpkgBuilderError>,
    >(
        writer: &mut BufWriter<TWriter>,
        provide_chunk_data: TChunkDataProvider,
        meta_chunks: &[MetaChunk<'_>],
        regular_chunks: &[&ModpkgChunkBuilder],
        chunk_path_indices: &HashMap<PathHash, u32>,
        layer_index_map: &HashMap<LayerHash, LayerIndex>,
        wad_indices: &HashMap<WadNameHash, WadIndex>,
    ) -> Result<Vec<ModpkgChunk>, ModpkgBuilderError> {
        let mut all_chunks = Self::process_meta_chunks(writer, meta_chunks, chunk_path_indices)?;
        let mut processed_regular_chunks = Self::process_chunks(
            regular_chunks,
            writer,
            provide_chunk_data,
            chunk_path_indices,
            layer_index_map,
            wad_indices,
        )?;

        all_chunks.append(&mut processed_regular_chunks);

        Ok(all_chunks)
    }

    /// Collect all non-meta chunks, sorted by WAD name then layer.
    ///
    /// This groups related chunks physically in the file,
    /// enabling more sequential I/O when reading all overrides for a WAD.
    fn collect_regular_chunks(
        &self,
        meta_path_hashes: &HashSet<PathHash>,
    ) -> Vec<&ModpkgChunkBuilder> {
        let mut regular_chunks: Vec<_> = self
            .chunks
            .values()
            .filter(|chunk| !meta_path_hashes.contains(&chunk.path_hash))
            .collect();
        regular_chunks.sort_by(|a, b| a.wad.cmp(&b.wad).then(a.layer.cmp(&b.layer)));

        regular_chunks
    }

    fn write_chunk_toc<W: io::Write + io::Seek>(
        writer: &mut W,
        chunk_toc_offset: u64,
        chunks: &[ModpkgChunk],
    ) -> Result<(), ModpkgBuilderError> {
        writer.seek(SeekFrom::Start(chunk_toc_offset))?;
        for chunk in chunks {
            chunk.write(writer)?;
        }
        Ok(())
    }

    /// The meta chunks this builder will write, in write order.
    ///
    /// This is the only place that decides which meta chunks exist and how each
    /// is stored; the metadata chunk is always present, the rest follow their
    /// content field.
    fn meta_chunks(&self) -> Result<Vec<MetaChunk<'_>>, ModpkgBuilderError> {
        let mut metadata_bytes = Vec::new();
        self.metadata.write(&mut metadata_bytes)?;

        let mut meta_chunks = vec![MetaChunk {
            path: METADATA_CHUNK_PATH,
            data: Cow::Owned(metadata_bytes),
            compression: ModpkgCompression::None,
        }];

        if let Some(thumbnail) = self.thumbnail.as_deref() {
            meta_chunks.push(MetaChunk {
                path: THUMBNAIL_CHUNK_PATH,
                data: Cow::Borrowed(thumbnail),
                compression: ModpkgCompression::None,
            });
        }

        if let Some(readme) = self.readme.as_deref() {
            meta_chunks.push(MetaChunk {
                path: README_CHUNK_PATH,
                data: Cow::Borrowed(readme),
                compression: ModpkgCompression::None,
            });
        }

        // License texts are boilerplate-heavy and compress well (a CC license
        // runs to tens of kilobytes), so the chunk requests Zstd. Short texts
        // that don't pay for it fall back to raw storage in `write_meta_chunk`.
        if let Some(license_text) = self.license_text.as_deref() {
            meta_chunks.push(MetaChunk {
                path: LICENSE_CHUNK_PATH,
                data: Cow::Borrowed(license_text),
                compression: ModpkgCompression::Zstd,
            });
        }

        Ok(meta_chunks)
    }

    fn process_meta_chunks<TWriter: io::Write + io::Seek>(
        writer: &mut BufWriter<TWriter>,
        meta_chunks: &[MetaChunk<'_>],
        chunk_path_indices: &HashMap<PathHash, u32>,
    ) -> Result<Vec<ModpkgChunk>, ModpkgBuilderError> {
        let mut processed = Vec::with_capacity(meta_chunks.len());

        for meta_chunk in meta_chunks {
            processed.push(Self::write_meta_chunk(
                meta_chunk.path_hash(),
                &meta_chunk.data,
                meta_chunk.compression,
                writer,
                chunk_path_indices,
            )?);
        }

        Ok(processed)
    }

    /// Write a meta chunk's data and return its TOC entry.
    ///
    /// `compression` is a request, not a guarantee: as with regular chunks, the
    /// data is stored raw when compressing it doesn't meaningfully pay, and the
    /// returned entry records the form actually stored.
    fn write_meta_chunk<TWriter: io::Write + io::Seek>(
        path_hash: PathHash,
        data: &[u8],
        compression: ModpkgCompression,
        writer: &mut BufWriter<TWriter>,
        chunk_path_indices: &HashMap<PathHash, u32>,
    ) -> Result<ModpkgChunk, ModpkgBuilderError> {
        let uncompressed_size = data.len();
        let uncompressed_checksum = xxh3_64(data);

        let (stored_data, compression) = Self::compress_chunk_data(data, compression)?;
        let compressed_size = stored_data.len();
        let compressed_checksum = xxh3_64(&stored_data);

        let data_offset = writer.stream_position()?;
        writer.write_all(&stored_data)?;

        Ok(ModpkgChunk {
            path_hash,
            data_offset,
            compression,
            compressed_size: compressed_size as u64,
            uncompressed_size: uncompressed_size as u64,
            compressed_checksum,
            uncompressed_checksum,
            path_index: *chunk_path_indices.get(&path_hash).unwrap_or(&0),
            layer_index: LayerIndex::NONE,
            wad_index: WadIndex::NONE,
        })
    }

    /// Maximum size of the compressed data relative to the original, in percent,
    /// for compression to be considered worthwhile. Chunks that don't compress
    /// below this threshold (already-compressed content like Vorbis audio or
    /// JPEG/WebP images) are stored uncompressed.
    const MAX_COMPRESSED_SIZE_PERCENT: u64 = 95;

    fn compress_chunk_data(
        data: &[u8],
        compression: ModpkgCompression,
    ) -> Result<(Vec<u8>, ModpkgCompression), ModpkgBuilderError> {
        match compression {
            ModpkgCompression::None => Ok((data.to_vec(), ModpkgCompression::None)),
            ModpkgCompression::Zstd => {
                let compressed = zstd::bulk::compress(data, 3)?;

                if (compressed.len() as u64) * 100
                    >= (data.len() as u64) * Self::MAX_COMPRESSED_SIZE_PERCENT
                {
                    Ok((data.to_vec(), ModpkgCompression::None))
                } else {
                    Ok((compressed, ModpkgCompression::Zstd))
                }
            }
        }
    }

    fn collect_unique_layers(
        chunks: &[&ModpkgChunkBuilder],
    ) -> (Vec<String>, HashMap<LayerHash, LayerIndex>) {
        let mut layers = Vec::new();
        let mut layer_indices = HashMap::new();
        for chunk in chunks {
            // Skip empty layer names (they represent chunks with no layer)
            if chunk.layer.is_empty() {
                continue;
            }
            let hash = LayerHash::from_name(&chunk.layer);
            layer_indices.entry(hash).or_insert_with(|| {
                let index = layers.len();
                layers.push(chunk.layer.clone());
                LayerIndex::new(index as u32)
            });
        }

        (layers, layer_indices)
    }

    fn collect_unique_paths(
        meta_chunks: &[MetaChunk<'_>],
        regular_chunks: &[&ModpkgChunkBuilder],
    ) -> (Vec<String>, HashMap<PathHash, u32>) {
        let mut paths = Vec::new();
        let mut path_indices = HashMap::new();

        // Collect paths from both meta chunks and regular chunks
        let meta_paths = meta_chunks
            .iter()
            .map(|meta| (meta.path_hash(), meta.path.to_string()));
        let regular_paths = regular_chunks
            .iter()
            .map(|chunk| (chunk.path_hash, chunk.path.clone()));

        for (path_hash, path) in meta_paths.chain(regular_paths) {
            path_indices.entry(path_hash).or_insert_with(|| {
                let index = paths.len();
                paths.push(path);
                index as u32
            });
        }

        (paths, path_indices)
    }

    fn collect_unique_wads(
        chunks: &[&ModpkgChunkBuilder],
    ) -> (Vec<String>, HashMap<WadNameHash, WadIndex>) {
        let mut wads = Vec::new();
        let mut wad_indices = HashMap::new();
        for chunk in chunks {
            // Skip empty wad names (they represent chunks with no wad)
            if chunk.wad.is_empty() {
                continue;
            }
            wad_indices
                .entry(WadNameHash::from_name(&chunk.wad))
                .or_insert_with(|| {
                    let index = wads.len();
                    wads.push(chunk.wad.clone());
                    WadIndex::new(index as u32)
                });
        }
        (wads, wad_indices)
    }

    fn validate_layers(
        defined_layers: &[ModpkgLayerBuilder],
        unique_layers: &[String],
    ) -> Result<(), ModpkgBuilderError> {
        // Check if defined layers have base layer
        if !defined_layers
            .iter()
            .any(|layer| layer.name == BASE_LAYER_NAME)
        {
            return Err(ModpkgBuilderError::MissingBaseLayer);
        }

        // Check if all unique layers are defined
        for layer in unique_layers {
            // Skip validation for empty layer names (they represent chunks with no layer)
            if layer.is_empty() {
                continue;
            }
            if !defined_layers.iter().any(|l| l.name == layer.as_str()) {
                return Err(ModpkgBuilderError::LayerNotFound(layer.to_string()));
            }
        }

        Ok(())
    }

    fn process_chunks<
        TWriter: io::Write + io::Seek,
        TChunkDataProvider: FnMut(&ModpkgChunkBuilder) -> Result<Vec<u8>, ModpkgBuilderError>,
    >(
        chunks: &[&ModpkgChunkBuilder],
        writer: &mut BufWriter<TWriter>,
        mut provide_chunk_data: TChunkDataProvider,
        chunk_path_indices: &HashMap<PathHash, u32>,
        layer_indices: &HashMap<LayerHash, LayerIndex>,
        wad_indices: &HashMap<WadNameHash, WadIndex>,
    ) -> Result<Vec<ModpkgChunk>, ModpkgBuilderError> {
        let mut final_chunks = Vec::new();

        // Identical chunk content is stored once: chunks whose data matches an
        // already written chunk point at the first copy's `data_offset`. The
        // stored form (and thus the recorded compression) is that of the first
        // occurrence, regardless of what compression later duplicates requested.
        let mut written_by_content: HashMap<ContentKey, (u64, u64, u64, ModpkgCompression)> =
            HashMap::new();

        // Entries sharing an identity register one chunk under several WADs,
        // so the format requires their content to be identical.
        let mut content_by_identity: HashMap<ChunkKey, (ContentKey, &str)> = HashMap::new();

        for chunk_builder in chunks {
            let uncompressed_data = provide_chunk_data(chunk_builder)?;
            let uncompressed_size = uncompressed_data.len();
            let uncompressed_checksum = xxh3_64(&uncompressed_data);

            let content_key: ContentKey = (uncompressed_checksum, uncompressed_size as u64);

            match content_by_identity.get(&chunk_builder.key()) {
                Some(&(first_content, first_wad)) => {
                    if first_content != content_key {
                        return Err(ModpkgBuilderError::InconsistentChunk {
                            path: chunk_builder.path.clone(),
                            layer: chunk_builder.layer.clone(),
                            first_wad: first_wad.to_string(),
                            second_wad: chunk_builder.wad.clone(),
                        });
                    }
                }
                None => {
                    content_by_identity.insert(
                        chunk_builder.key(),
                        (content_key, chunk_builder.wad.as_str()),
                    );
                }
            }
            let (data_offset, compressed_size, compressed_checksum, compression) =
                match written_by_content.get(&content_key) {
                    Some(&existing) => existing,
                    None => {
                        let (compressed_data, compression) = Self::compress_chunk_data(
                            &uncompressed_data,
                            chunk_builder.compression,
                        )?;

                        let compressed_size = compressed_data.len() as u64;
                        let compressed_checksum = xxh3_64(&compressed_data);

                        let data_offset = writer.stream_position()?;
                        writer.write_all(&compressed_data)?;

                        let written = (
                            data_offset,
                            compressed_size,
                            compressed_checksum,
                            compression,
                        );
                        written_by_content.insert(content_key, written);
                        written
                    }
                };

            let path_hash = chunk_builder.path_hash;
            let layer_index = if chunk_builder.layer.is_empty() {
                LayerIndex::NONE
            } else {
                layer_indices
                    .get(&LayerHash::from_name(&chunk_builder.layer))
                    .copied()
                    .unwrap_or(LayerIndex::NONE)
            };
            let wad_index = if chunk_builder.wad.is_empty() {
                WadIndex::NONE
            } else {
                wad_indices
                    .get(&WadNameHash::from_name(&chunk_builder.wad))
                    .copied()
                    .unwrap_or(WadIndex::NONE)
            };

            let chunk = ModpkgChunk {
                path_hash,
                data_offset,
                compression,
                compressed_size,
                uncompressed_size: uncompressed_size as u64,
                compressed_checksum,
                uncompressed_checksum,
                path_index: *chunk_path_indices.get(&path_hash).unwrap_or(&0),
                layer_index,
                wad_index,
            };

            final_chunks.push(chunk);
        }

        Ok(final_chunks)
    }
}

impl ModpkgChunkBuilder {
    const DEFAULT_LAYER: &'static str = "base";

    /// Create a new chunk builder with the default layer.
    pub fn new() -> Self {
        Self {
            path_hash: PathHash::new(0),
            path: String::new(),
            compression: ModpkgCompression::None,
            layer: Self::DEFAULT_LAYER.to_string(),
            wad: String::new(),
        }
    }

    /// Set the path of the chunk (input path is case insensitive).
    ///
    /// The path is normalized into a [`ChunkPath`] and hashed from that form.
    pub fn with_path(mut self, path: &str) -> Self {
        let path = ChunkPath::new(path);
        self.path_hash = path.hash();
        self.path = path.into_string();
        self
    }

    /// Set the path hash from a hex-encoded chunk name that represents the actual path hash.
    ///
    /// The input must have a base filename of exactly 16 hexadecimal characters. Any number of
    /// extensions after the base is allowed (only the base is parsed). The `0x` prefix is NOT
    /// allowed.
    /// The builder stores the provided (lowercased) string as the display path and parses the
    /// base as hexadecimal for the `path_hash`.
    pub fn with_hashed_chunk_name(mut self, hashed_name: &str) -> Result<Self, ModpkgBuilderError> {
        let display_path = hashed_name.to_lowercase();

        let filename = Path::new(&display_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&display_path);

        self.path_hash = PathHash::from_hex_name(filename)
            .ok_or_else(|| ModpkgBuilderError::InvalidChunkName(display_path.clone()))?;
        self.path = display_path;

        Ok(self)
    }

    /// Request a compression type for the chunk's data.
    ///
    /// This is an upper bound rather than a guarantee: the builder stores the
    /// chunk uncompressed when compression does not meaningfully reduce its
    /// size (e.g. already-compressed audio or image formats). The chunk's TOC
    /// entry always records the form actually stored.
    pub fn with_compression(mut self, compression: ModpkgCompression) -> Self {
        self.compression = compression;
        self
    }

    pub fn with_layer(mut self, layer: &str) -> Self {
        self.layer = layer.to_string();
        self
    }

    /// Set the WAD association for this chunk.
    ///
    /// This enables efficient WAD-based lookups via the secondary index.
    pub fn with_wad(mut self, wad: &str) -> Self {
        self.wad = wad.to_lowercase();
        self
    }

    /// The hash of the chunk's canonical path.
    pub fn path_hash(&self) -> PathHash {
        self.path_hash
    }

    /// The chunk's canonical path (or hex chunk name).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The compression requested for the chunk's data.
    pub fn compression(&self) -> ModpkgCompression {
        self.compression
    }

    /// The name of the layer the chunk belongs to.
    pub fn layer(&self) -> &str {
        &self.layer
    }

    /// The lowercased name of the chunk's WAD, or `""` without a WAD.
    pub fn wad(&self) -> &str {
        &self.wad
    }

    /// Compute the identity of this chunk.
    ///
    /// This mirrors how chunks are keyed in the final [`Modpkg`](crate::Modpkg).
    pub fn key(&self) -> ChunkKey {
        let layer_hash = if self.layer.is_empty() {
            LayerHash::NONE
        } else {
            LayerHash::from_name(&self.layer)
        };
        ChunkKey::new(self.path_hash, layer_hash)
    }

    /// The hash of this chunk's WAD name, or [`WadNameHash::NONE`] without a WAD.
    pub fn wad_hash(&self) -> WadNameHash {
        if self.wad.is_empty() {
            WadNameHash::NONE
        } else {
            WadNameHash::from_name(&self.wad)
        }
    }

    /// Compute the storage key for this chunk: its identity plus its WAD.
    ///
    /// Unlike [`key`](Self::key), this tells apart entries that register one
    /// chunk identity under several WADs.
    pub fn full_key(&self) -> (ChunkKey, WadNameHash) {
        (self.key(), self.wad_hash())
    }
}

// The `with_*` meta setters below only assign a field. The corresponding chunk
// is derived at build time by [`ModpkgBuilder::meta_chunks`].
impl ModpkgBuilder {
    /// Set the metadata for the builder.
    pub fn with_metadata(mut self, metadata: ModpkgMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set the readme for the builder.
    ///
    /// The bytes are not decoded, so a readme that is not valid UTF-8 survives
    /// the round trip unchanged.
    pub fn with_readme(mut self, readme: impl Into<Vec<u8>>) -> Self {
        self.readme = Some(readme.into());
        self
    }

    /// Set the license text for the builder.
    ///
    /// The bytes are not decoded: a license is a legal document, and a
    /// `LICENSE` saved in Latin-1 must not come back out with its copyright
    /// symbol replaced.
    ///
    /// The chunk is stored compressed; see [`meta_chunks`](Self::meta_chunks).
    pub fn with_license_text(mut self, license_text: impl Into<Vec<u8>>) -> Self {
        self.license_text = Some(license_text.into());
        self
    }

    /// Set the thumbnail for the builder.
    pub fn with_thumbnail(mut self, thumbnail: impl Into<Vec<u8>>) -> Self {
        self.thumbnail = Some(thumbnail.into());
        self
    }
}

impl ModpkgLayerBuilder {
    /// Create a layer builder, validating `name` as a [`Slug`].
    pub fn new(name: impl AsRef<str>) -> Result<Self, ModpkgBuilderError> {
        Ok(Self {
            name: Slug::new(name)?,
            priority: 0,
        })
    }

    /// Create a layer builder from an already-validated name.
    pub fn from_slug(name: Slug) -> Self {
        Self { name, priority: 0 }
    }

    pub fn with_name(mut self, name: impl AsRef<str>) -> Result<Self, ModpkgBuilderError> {
        self.name = Slug::new(name)?;
        Ok(self)
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// The base layer, whose name is always valid.
    pub fn base() -> Self {
        Self {
            name: Slug::base(),
            priority: 0,
        }
    }

    /// The layer's name.
    pub fn name(&self) -> &Slug {
        &self.name
    }

    /// The layer's priority.
    pub fn priority(&self) -> i32 {
        self.priority
    }
}

#[cfg(test)]
mod tests {
    use crate::{Modpkg, ModpkgError, ModpkgLayer};

    use super::*;

    use std::io::Cursor;

    #[test]
    fn test_modpkg_builder() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let builder = ModpkgBuilder::default()
            .with_metadata(ModpkgMetadata::default())
            .with_layer(ModpkgLayerBuilder::new("base").unwrap().with_priority(0))
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("test.png")
                    .with_compression(ModpkgCompression::Zstd)
                    .with_layer("base"),
            );

        builder
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 100]))
            .expect("Failed to build Modpkg");

        // Reset cursor and verify the file was created
        cursor.set_position(0);

        let modpkg = Modpkg::mount_from_reader(&mut cursor).unwrap();

        // Now we have 2 chunks: metadata + test.png
        assert_eq!(modpkg.chunks().len(), 2);

        let chunk = modpkg
            .chunks()
            .get(&ChunkKey::new(
                ChunkPath::new("test.png").hash(),
                LayerHash::from_name("base"),
            ))
            .unwrap();

        assert_eq!(
            modpkg.chunk_paths().get(&ChunkPath::new("test.png").hash()),
            Some(&"test.png".to_string())
        );

        assert_eq!(chunk.compression, ModpkgCompression::Zstd);
        assert_eq!(chunk.uncompressed_size, 100);
        assert_eq!(chunk.compressed_size, 17);
        assert_eq!(chunk.uncompressed_checksum, xxh3_64(&[0xAA; 100]));
        // Meta chunk paths are registered first, so the metadata chunk holds
        // index 0 and content paths follow.
        assert_eq!(chunk.path_index, 1);

        assert_eq!(modpkg.layers().len(), 1);
        assert_eq!(
            modpkg.layers().get(&LayerHash::from_name("base")),
            Some(&ModpkgLayer {
                name: "base".to_string(),
                priority: 0,
            })
        );
    }

    #[test]
    fn test_with_hashed_chunk_name() {
        // Test with an extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("abcdef1234567890.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), PathHash::new(0xabcdef1234567890));
        assert_eq!(chunk.path(), "abcdef1234567890.dds");

        // Test with an extension (no 0x prefix)
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("fedcba9876543210.txt")
            .unwrap();
        assert_eq!(chunk.path_hash(), PathHash::new(0xfedcba9876543210));
        assert_eq!(chunk.path(), "fedcba9876543210.txt");

        // Test with an extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("1234abc456def789.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), PathHash::new(0x1234abc456def789));
        assert_eq!(chunk.path(), "1234abc456def789.dds");

        // Test without extension
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("789def0011223344")
            .unwrap();
        assert_eq!(chunk.path_hash(), PathHash::new(0x789def0011223344));
        assert_eq!(chunk.path(), "789def0011223344");

        // Test invalid hex should fail
        assert!(ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("not_hex.bin")
            .is_err());

        // Multiple extensions are allowed as long as base is valid
        let chunk = ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("abcdef1234567890.texture.dds")
            .unwrap();
        assert_eq!(chunk.path_hash(), PathHash::new(0xabcdef1234567890));

        // 0x prefix should fail
        assert!(ModpkgChunkBuilder::new()
            .with_hashed_chunk_name("0xabcdef1234567890.dds")
            .is_err());
    }

    #[test]
    fn incompressible_chunk_falls_back_to_raw_storage() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        // Xorshift-generated noise doesn't compress, so the builder must
        // ignore the requested Zstd compression and store the bytes raw.
        let mut state = 0x9E3779B97F4A7C15u64;
        let noise: Vec<u8> = (0..4096)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("noise.bin")
                    .with_compression(ModpkgCompression::Zstd)
                    .with_layer("base"),
            );

        let noise_clone = noise.clone();
        builder
            .build_to_writer(&mut cursor, move |_| Ok(noise_clone.clone()))
            .expect("Failed to build Modpkg");

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let chunk = *modpkg
            .chunks()
            .get(&ChunkKey::new(
                ChunkPath::new("noise.bin").hash(),
                LayerHash::from_name("base"),
            ))
            .unwrap();
        assert_eq!(chunk.compression, ModpkgCompression::None);
        assert_eq!(chunk.compressed_size, chunk.uncompressed_size);

        let loaded = modpkg
            .load_chunk_decompressed_by_path("noise.bin", Some("base"))
            .unwrap();
        assert_eq!(&loaded[..], &noise[..]);
    }

    #[test]
    fn identical_chunk_content_is_stored_once() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        let shared_data = vec![0xAB; 10_000];
        let unique_data = vec![0xCD; 10_000];

        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("a.bin")
                    .with_compression(ModpkgCompression::Zstd)
                    .with_layer("base"),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("b.bin")
                    .with_compression(ModpkgCompression::Zstd)
                    .with_layer("base"),
            )
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("c.bin")
                    .with_compression(ModpkgCompression::Zstd)
                    .with_layer("base"),
            );

        let (shared_clone, unique_clone) = (shared_data.clone(), unique_data.clone());
        builder
            .build_to_writer(&mut cursor, move |chunk| {
                if chunk.path() == "c.bin" {
                    Ok(unique_clone.clone())
                } else {
                    Ok(shared_clone.clone())
                }
            })
            .expect("Failed to build Modpkg");

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        let base = LayerHash::from_name("base");
        let a = *modpkg
            .chunks()
            .get(&ChunkKey::new(ChunkPath::new("a.bin").hash(), base))
            .unwrap();
        let b = *modpkg
            .chunks()
            .get(&ChunkKey::new(ChunkPath::new("b.bin").hash(), base))
            .unwrap();
        let c = *modpkg
            .chunks()
            .get(&ChunkKey::new(ChunkPath::new("c.bin").hash(), base))
            .unwrap();

        // a and b share content: written once, both TOC entries point at it
        assert_eq!(a.data_offset, b.data_offset);
        assert_eq!(a.compressed_size, b.compressed_size);
        assert_eq!(a.compressed_checksum, b.compressed_checksum);

        // c has different content and its own data
        assert_ne!(c.data_offset, a.data_offset);
        assert_ne!(c.uncompressed_checksum, a.uncompressed_checksum);

        // All three decode to the bytes their TOC entries promise
        let loaded_a = modpkg
            .load_chunk_decompressed_by_path("a.bin", Some("base"))
            .unwrap();
        let loaded_b = modpkg
            .load_chunk_decompressed_by_path("b.bin", Some("base"))
            .unwrap();
        let loaded_c = modpkg
            .load_chunk_decompressed_by_path("c.bin", Some("base"))
            .unwrap();
        assert_eq!(&loaded_a[..], &shared_data[..]);
        assert_eq!(&loaded_b[..], &shared_data[..]);
        assert_eq!(&loaded_c[..], &unique_data[..]);
    }

    /// A chunk builder for `data/shared.bin` in the base layer of `wad`.
    fn shared_chunk(wad: &str) -> ModpkgChunkBuilder {
        ModpkgChunkBuilder::new()
            .with_path("data/shared.bin")
            .with_compression(ModpkgCompression::Zstd)
            .with_layer("base")
            .with_wad(wad)
    }

    /// One `(path, layer)` identity under two WADs: the archive stores
    /// duplicate TOC records that collapse into one `chunks` entry on mount,
    /// while both WAD groups keep the chunk's key.
    #[test]
    fn shared_chunk_across_wads_mounts_under_both() {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(shared_chunk("aatrox.wad.client"))
            .with_chunk(shared_chunk("ahri.wad.client"))
            .build_to_writer(&mut cursor, |_| Ok(vec![0xCC; 64]))
            .expect("Failed to build Modpkg");

        cursor.set_position(0);
        let mut modpkg = Modpkg::mount_from_reader(cursor).unwrap();

        // Metadata + one entry for the shared identity.
        assert_eq!(modpkg.chunks().len(), 2);

        let key = ChunkKey::new(
            ChunkPath::new("data/shared.bin").hash(),
            LayerHash::from_name("base"),
        );
        let layer_index = modpkg.layer_index("base").unwrap();
        for wad in ["aatrox.wad.client", "ahri.wad.client"] {
            let wad_index = modpkg.wad_index(wad).expect("wad in table");
            assert_eq!(
                modpkg.chunks_for_wad_layer(wad_index, layer_index),
                [key],
                "{wad} should hold the shared chunk"
            );
        }

        let loaded = modpkg
            .load_chunk_decompressed_by_path("data/shared.bin", Some("base"))
            .unwrap();
        assert_eq!(&loaded[..], &[0xCC; 64]);
    }

    #[test]
    fn inconsistent_shared_chunk_content_fails_the_build() {
        let mut cursor = Cursor::new(Vec::new());

        // Each WAD supplies different bytes for the shared identity.
        let err = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(shared_chunk("aatrox.wad.client"))
            .with_chunk(shared_chunk("ahri.wad.client"))
            .build_to_writer(&mut cursor, |chunk| Ok(chunk.wad().as_bytes().to_vec()))
            .unwrap_err();

        assert!(
            matches!(
                err,
                ModpkgBuilderError::InconsistentChunk {
                    ref path,
                    ref layer,
                    ref first_wad,
                    ref second_wad,
                } if path == "data/shared.bin"
                    && layer == "base"
                    && first_wad == "aatrox.wad.client"
                    && second_wad == "ahri.wad.client"
            ),
            "Expected InconsistentChunk, got: {err}"
        );
    }

    /// A hand-tampered archive whose duplicate records disagree must not
    /// mount: which record wins would depend on read order.
    #[test]
    fn mount_rejects_inconsistent_duplicate_records() {
        let mut cursor = Cursor::new(Vec::new());

        ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(shared_chunk("aatrox.wad.client"))
            .with_chunk(shared_chunk("ahri.wad.client"))
            .build_to_writer(&mut cursor, |_| Ok(vec![0xCC; 64]))
            .expect("Failed to build Modpkg");

        let mut bytes = cursor.into_inner();

        // Find the two TOC records by their leading path hash and corrupt the
        // second record's uncompressed checksum (bytes 41..49 of the record).
        let path_hash = ChunkPath::new("data/shared.bin").hash();
        let needle = path_hash.value().to_le_bytes();
        let records: Vec<usize> = (0..=bytes.len() - 8)
            .filter(|&pos| bytes[pos..pos + 8] == needle)
            .collect();
        assert_eq!(records.len(), 2, "expected exactly two TOC records");
        for byte in &mut bytes[records[1] + 41..records[1] + 49] {
            *byte ^= 0xFF;
        }

        let err = Modpkg::mount_from_reader(Cursor::new(bytes)).unwrap_err();
        assert!(
            matches!(err, ModpkgError::ChunksInconsistent(hash) if hash == path_hash),
            "Expected ChunksInconsistent, got: {err}"
        );
    }

    /// The builder used to accept any string, so a caller bypassing the packer
    /// could produce a package with a layer name the packer would reject.
    #[test]
    fn layer_builder_rejects_names_the_packer_would_reject() {
        for name in ["", "High Res", "-leading", "trailing-"] {
            assert!(
                ModpkgLayerBuilder::new(name).is_err(),
                "{name} should be rejected"
            );
        }

        assert!(ModpkgLayerBuilder::new("high-res").is_ok());
    }

    #[test]
    fn with_path_normalizes_backslashes() {
        let chunk = ModpkgChunkBuilder::new()
            .with_path("ASSETS\\Characters\\Aatrox\\Skins\\Base\\Aatrox.dds");

        assert_eq!(chunk.path, "assets/characters/aatrox/skins/base/aatrox.dds");

        // Hash should match the normalized forward-slash version
        let forward =
            ModpkgChunkBuilder::new().with_path("assets/characters/aatrox/skins/base/aatrox.dds");

        assert_eq!(chunk.path_hash(), forward.path_hash());
    }

    #[test]
    fn roundtrip_backslash_paths_normalized() {
        let scratch = Vec::new();
        let mut cursor = Cursor::new(scratch);

        // Build with a path that has backslashes (simulates Windows glob output)
        let builder = ModpkgBuilder::default()
            .with_layer(ModpkgLayerBuilder::base())
            .with_chunk(
                ModpkgChunkBuilder::new()
                    .with_path("ASSETS\\Characters\\Aatrox\\Aatrox.dds")
                    .with_compression(ModpkgCompression::None)
                    .with_layer("base"),
            );

        builder
            .build_to_writer(&mut cursor, |_| Ok(vec![0xBB; 50]))
            .expect("Failed to build Modpkg");

        cursor.set_position(0);
        let modpkg = Modpkg::mount_from_reader(&mut cursor).unwrap();

        // Stored path must be normalized
        let normalized = "assets/characters/aatrox/aatrox.dds";
        let path_hash = ChunkPath::new(normalized).hash();

        assert_eq!(
            modpkg.chunk_paths().get(&path_hash),
            Some(&normalized.to_string()),
            "chunk_paths should contain the normalized path"
        );

        // Chunk should be findable by the normalized hash
        assert!(
            modpkg
                .chunks()
                .contains_key(&ChunkKey::new(path_hash, LayerHash::from_name("base"))),
            "chunk should be retrievable with normalized path hash"
        );
    }
}
