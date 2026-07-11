use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};

use super::cose::{self, TokenError, claim};

/// File entry in a package (name hash, checksums, content digest).
///
/// The xxh3 checksums mirror the WAD TOC and are what the runtime merged-WAD
/// check compares against; they are NOT collision resistant. `digest_decompressed`
/// is the cryptographic binding to the actual content: verifiers hash the
/// decompressed chunk at install/overlay-build time.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FileEntry {
    /// Hash of file name/path (xxh64).
    pub name_hash: u64,

    /// xxh3 checksum of compressed data as it appears in the merged WAD.
    pub checksum_compressed: u64,

    /// xxh3 checksum of uncompressed data.
    pub checksum_uncompressed: u64,

    /// SHA-256 of the decompressed data.
    pub digest_decompressed: [u8; 32],
}

impl FileEntry {
    /// Packed wire size: 3x u64 LE + 32-byte digest.
    pub const SIZE: usize = 56;

    fn pack(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..8].copy_from_slice(&self.name_hash.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.checksum_compressed.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.checksum_uncompressed.to_le_bytes());
        bytes[24..56].copy_from_slice(&self.digest_decompressed);
        bytes
    }

    fn unpack(bytes: &[u8; Self::SIZE]) -> Self {
        Self {
            name_hash: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            checksum_compressed: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            checksum_uncompressed: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            digest_decompressed: bytes[24..56].try_into().unwrap(),
        }
    }
}

/// Inputs for sealing a new [`Statement`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementParams<'a> {
    /// Optional name of the WAD the files belong to (e.g. `Aatrox.wad.client`;
    /// non-empty ASCII, < 128 bytes).
    pub wad_name: Option<&'a str>,
    /// Optional SHA-256 of the TOC of the WAD build the files were validated
    /// against, pinning the statement to one exact game build.
    pub wad_toc_digest: Option<[u8; 32]>,
    /// File entries, strictly sorted by `name_hash`, no duplicates.
    pub entries: &'a [FileEntry],
}

/// The content token: a set of package files, with two-tier checksums,
/// optionally bound to the WAD they belong to.
///
/// Bare token with `typ = "ltk/statement"`. Statements are **unsigned**:
/// their identity is the SHA-256 of the sealed bytes ([`Self::token_hash`]),
/// and they are trusted only once a platform attests that hash
/// (`trust::PlatformTrust::attest`). The WAD bindings are advisory scoping;
/// attestation binds the statement bytes wholesale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    token: cose::BareToken,
    wad_name: Option<String>,
    wad_toc_digest: Option<[u8; 32]>,
    entries: Vec<FileEntry>,
}

impl Statement {
    pub const TYP: &'static str = "ltk/statement";

    /// Build and seal a new statement. No key is involved; the sealed bytes
    /// are the identity.
    pub fn seal(params: &StatementParams) -> Result<Self, TokenError> {
        if let Some(name) = params.wad_name {
            cose::validate_name(name, "wad_name")?;
        }
        if !params
            .entries
            .windows(2)
            .all(|w| w[0].name_hash < w[1].name_hash)
        {
            return Err(TokenError::ClaimInvalid("entries must be sorted"));
        }
        let mut packed = Vec::with_capacity(params.entries.len() * FileEntry::SIZE);
        for entry in params.entries {
            packed.extend_from_slice(&entry.pack());
        }
        let mut builder =
            ClaimsSetBuilder::new().private_claim(claim::FILE_ENTRIES, Value::Bytes(packed));
        if let Some(name) = params.wad_name {
            builder = builder.private_claim(claim::WAD_NAME, Value::Text(name.to_owned()));
        }
        if let Some(digest) = params.wad_toc_digest {
            builder = builder.private_claim(claim::WAD_TOC_DIGEST, Value::Bytes(digest.to_vec()));
        }
        let raw = cose::seal_bare(Self::TYP, builder.build())?;
        Self::from_bytes(&raw)
    }

    /// Parse and structurally validate a statement.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenError> {
        let (token, claims) = cose::parse_bare(Self::TYP, bytes)?;
        let wad_name = match cose::private_claim(&claims, claim::WAD_NAME) {
            Some(value) => {
                let name = value
                    .as_text()
                    .ok_or(TokenError::ClaimInvalid("wad_name"))?;
                cose::validate_name(name, "wad_name")?;
                Some(name.to_owned())
            }
            None => None,
        };
        let wad_toc_digest = cose::private_claim(&claims, claim::WAD_TOC_DIGEST)
            .map(|value| cose::claim_bstr_exact::<32>(value, "wad_toc_digest"))
            .transpose()?;
        let packed = cose::private_claim(&claims, claim::FILE_ENTRIES)
            .and_then(|value| value.as_bytes())
            .ok_or(TokenError::ClaimMissing("entries"))?;
        if !packed.len().is_multiple_of(FileEntry::SIZE) {
            return Err(TokenError::ClaimInvalid("entries"));
        }
        let entries: Vec<FileEntry> = packed
            .chunks_exact(FileEntry::SIZE)
            .map(|chunk| FileEntry::unpack(chunk.try_into().unwrap()))
            .collect();
        if !entries.windows(2).all(|w| w[0].name_hash < w[1].name_hash) {
            return Err(TokenError::ClaimInvalid("entries must be sorted"));
        }
        Ok(Self {
            token,
            wad_name,
            wad_toc_digest,
            entries,
        })
    }

    /// Exact serialized bytes of the token (what the identity hash covers).
    pub fn as_bytes(&self) -> &[u8] {
        &self.token.raw
    }

    /// SHA-256 of the serialized token: the identity attestations and set
    /// files endorse.
    pub fn token_hash(&self) -> [u8; 32] {
        self.token.checksum
    }

    /// Name of the WAD the files belong to, if bound.
    pub fn wad_name(&self) -> Option<&str> {
        self.wad_name.as_deref()
    }

    /// SHA-256 of the TOC of the WAD build the statement targets, if bound.
    pub fn wad_toc_digest(&self) -> Option<&[u8; 32]> {
        self.wad_toc_digest.as_ref()
    }

    /// File entries, sorted by `name_hash`.
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// Find an entry by file name hash.
    pub fn get_by_name_hash(&self, name_hash: u64) -> Option<&FileEntry> {
        self.entries
            .binary_search_by_key(&name_hash, |e| e.name_hash)
            .ok()
            .map(|i| &self.entries[i])
    }
}
