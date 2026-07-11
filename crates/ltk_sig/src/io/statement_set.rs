//! Set files and inclusion proofs: the membership proof formats.
//!
//! One static file per set: the sorted, unique 32-byte SHA-256 hashes of
//! every statement the platform currently endorses in that partition
//! (`n×32` bytes, no header). The set's **digest** — the value the root's
//! `roots` map carries and the content address the file is fetched at — is
//! the RFC 6962 Merkle tree hash over those hashes as leaf inputs, in
//! order: `leaf = SHA-256(0x00 ‖ statement hash)`, `node = SHA-256(0x01 ‖
//! left ‖ right)`, subtrees split at the largest power of two smaller than
//! the leaf count, and the empty set hashing to `SHA-256("")`. The encoding
//! is canonical (packed, sorted, unique), so the digest pins the exact file
//! bytes just as a flat hash would.
//!
//! Membership under that digest can be shown two ways:
//!
//! - **The whole set file** ([`StatementSet`]): parse, recompute the tree
//!   hash, binary-search the statement hash. This is the manager's form —
//!   distribution ships whole files, and the manager needs them anyway
//!   (proofs can only be *extracted* from a full set).
//! - **An inclusion proof** ([`InclusionProof`]): the RFC 6962 audit path
//!   from one statement hash to the digest, `32·⌈log₂ n⌉` bytes regardless
//!   of set size. Extracted at overlay build time ([`StatementSet::prove`])
//!   and embedded alongside the statement, so a game-side verifier checks
//!   statements against the signed root ([`verify_inclusion`]) without ever
//!   reading a set file.
//!
//! Hash membership endorses the exact statement bytes, so either form is a
//! complete endorsement by itself.

use thiserror::Error;

/// Maximum accepted size of a set file (matches the token size cap).
pub const MAX_SET_LEN: usize = super::cose::MAX_TOKEN_LEN;

#[derive(Debug, Error, miette::Diagnostic, PartialEq, Eq, Clone)]
pub enum StatementSetError {
    #[error("Statement set is too large: {0} bytes")]
    TooLarge(usize),

    #[error("Statement set is malformed: {0}")]
    Malformed(&'static str),

    #[error("Inclusion proof is malformed: {0}")]
    ProofMalformed(&'static str),
}

/// A parsed set file: sorted, unique statement token hashes, identified by
/// the Merkle tree hash over them (see the module docs for the derivation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSet {
    hashes: Vec<[u8; 32]>,
    digest: [u8; 32],
}

impl StatementSet {
    /// Build a set from statement hashes (strictly ascending, no duplicates).
    pub fn from_hashes(hashes: Vec<[u8; 32]>) -> Result<Self, StatementSetError> {
        if hashes.len() * 32 > MAX_SET_LEN {
            return Err(StatementSetError::TooLarge(hashes.len() * 32));
        }
        if !hashes.windows(2).all(|w| w[0] < w[1]) {
            return Err(StatementSetError::Malformed("hashes must be sorted"));
        }
        let digest = tree_hash(&hashes);
        Ok(Self { hashes, digest })
    }

    /// Parse a set file. The encoding is canonical (packed, sorted, unique),
    /// so [`Self::to_bytes`] reproduces the input byte-for-byte and
    /// [`Self::digest`] pins the exact received bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StatementSetError> {
        if bytes.len() > MAX_SET_LEN {
            return Err(StatementSetError::TooLarge(bytes.len()));
        }
        if !bytes.len().is_multiple_of(32) {
            return Err(StatementSetError::Malformed("length not a multiple of 32"));
        }
        let hashes: Vec<[u8; 32]> = bytes
            .chunks_exact(32)
            .map(|chunk| <[u8; 32]>::try_from(chunk).unwrap())
            .collect();
        if !hashes.windows(2).all(|w| w[0] < w[1]) {
            return Err(StatementSetError::Malformed("hashes must be sorted"));
        }
        let digest = tree_hash(&hashes);
        Ok(Self { hashes, digest })
    }

    /// Serialize to the packed on-wire form.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hashes.concat()
    }

    /// The Merkle tree hash over the statement hashes: the identity the
    /// root vouches for and the content address the set file is fetched at.
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Check whether a statement token hash is endorsed by this set.
    pub fn contains(&self, token_hash: &[u8; 32]) -> bool {
        self.hashes.binary_search(token_hash).is_ok()
    }

    /// Extract the inclusion proof for a statement hash, if the set
    /// endorses it. The proof verifies against [`Self::digest`] via
    /// [`verify_inclusion`].
    pub fn prove(&self, token_hash: &[u8; 32]) -> Option<InclusionProof> {
        let index = self.hashes.binary_search(token_hash).ok()?;
        let mut path = Vec::new();
        audit_path(&self.hashes, index, &mut path);
        Some(InclusionProof {
            leaf_index: index as u64,
            tree_size: self.hashes.len() as u64,
            path,
        })
    }

    /// Endorsed statement hashes, sorted.
    pub fn hashes(&self) -> &[[u8; 32]] {
        &self.hashes
    }

    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }
}

/// An RFC 6962 audit path binding one statement hash to a set digest: the
/// compact evidence form for verifiers that do not hold set files.
///
/// A proof carries no authority of its own — it is unsigned data, judged
/// only by whether it connects a statement hash to a digest the platform's
/// current root signs ([`verify_inclusion`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InclusionProof {
    /// Index of the statement hash within the sorted set.
    pub leaf_index: u64,
    /// Number of hashes in the set the proof was extracted from.
    pub tree_size: u64,
    /// Sibling subtree hashes, leaf-adjacent first.
    pub path: Vec<[u8; 32]>,
}

impl InclusionProof {
    /// Packed wire size of the fixed header (`leaf_index`, `tree_size`).
    const HEADER: usize = 16;

    /// Maximum audit path length (a path of 63 covers any tree a u64 can
    /// index; real sets are capped far below by [`MAX_SET_LEN`]).
    pub const MAX_PATH: usize = 63;

    /// Serialize to the packed on-wire form:
    /// `leaf_index (u64 LE) ‖ tree_size (u64 LE) ‖ path (n×32)`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::HEADER + self.path.len() * 32);
        bytes.extend_from_slice(&self.leaf_index.to_le_bytes());
        bytes.extend_from_slice(&self.tree_size.to_le_bytes());
        for node in &self.path {
            bytes.extend_from_slice(node);
        }
        bytes
    }

    /// Parse a packed inclusion proof. Validates shape only; whether the
    /// proof actually binds anything is [`verify_inclusion`]'s judgement.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StatementSetError> {
        if bytes.len() < Self::HEADER {
            return Err(StatementSetError::ProofMalformed("header truncated"));
        }
        let (header, path_bytes) = bytes.split_at(Self::HEADER);
        if !path_bytes.len().is_multiple_of(32) {
            return Err(StatementSetError::ProofMalformed(
                "path length not a multiple of 32",
            ));
        }
        let leaf_index = u64::from_le_bytes(header[0..8].try_into().unwrap());
        let tree_size = u64::from_le_bytes(header[8..16].try_into().unwrap());
        if leaf_index >= tree_size {
            return Err(StatementSetError::ProofMalformed("leaf index out of range"));
        }
        let path: Vec<[u8; 32]> = path_bytes
            .chunks_exact(32)
            .map(|chunk| <[u8; 32]>::try_from(chunk).unwrap())
            .collect();
        if path.len() > Self::MAX_PATH {
            return Err(StatementSetError::ProofMalformed("path too long"));
        }
        Ok(Self {
            leaf_index,
            tree_size,
            path,
        })
    }
}

/// Verify that `statement_hash` is a member of the set whose Merkle tree
/// hash is `digest`, per the audit-path check of RFC 9162 §2.1.3.2.
///
/// `digest` must come from a trusted source (the platform's current signed
/// root) — the proof itself is untrusted input and a forged one can only
/// fail.
pub fn verify_inclusion(
    digest: &[u8; 32],
    statement_hash: &[u8; 32],
    proof: &InclusionProof,
) -> bool {
    if proof.leaf_index >= proof.tree_size {
        return false;
    }
    let mut node = proof.leaf_index;
    let mut last = proof.tree_size - 1;
    let mut hash = leaf_hash(statement_hash);
    for sibling in &proof.path {
        if last == 0 {
            return false; // path longer than the tree is deep
        }
        if node & 1 == 1 || node == last {
            hash = node_hash(sibling, &hash);
            if node & 1 == 0 {
                // A right-edge node with no sibling at this level: skip
                // levels until it has one.
                loop {
                    node >>= 1;
                    last >>= 1;
                    if node & 1 == 1 || node == 0 {
                        break;
                    }
                }
            }
        } else {
            hash = node_hash(&hash, sibling);
        }
        node >>= 1;
        last >>= 1;
    }
    last == 0 && hash == *digest
}

// ------------------------------------------------- RFC 6962 tree hashing

fn leaf_hash(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update([0x00]);
    hasher.update(data);
    hasher.finalize().into()
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update([0x01]);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// Split point of an RFC 6962 tree over `n >= 2` leaves: the largest power
/// of two smaller than `n`.
fn split_point(n: usize) -> usize {
    1 << (n - 1).ilog2()
}

/// RFC 6962 Merkle tree hash over ordered leaf inputs.
fn tree_hash<L: AsRef<[u8]>>(leaves: &[L]) -> [u8; 32] {
    use sha2::Digest;
    match leaves {
        [] => sha2::Sha256::digest([]).into(),
        [leaf] => leaf_hash(leaf.as_ref()),
        _ => {
            let k = split_point(leaves.len());
            node_hash(&tree_hash(&leaves[..k]), &tree_hash(&leaves[k..]))
        }
    }
}

/// RFC 6962 audit path for `leaves[index]`, collected leaf-adjacent first.
fn audit_path<L: AsRef<[u8]>>(leaves: &[L], index: usize, out: &mut Vec<[u8; 32]>) {
    if leaves.len() <= 1 {
        return;
    }
    let k = split_point(leaves.len());
    if index < k {
        audit_path(&leaves[..k], index, out);
        out.push(tree_hash(&leaves[k..]));
    } else {
        audit_path(&leaves[k..], index - k, out);
        out.push(tree_hash(&leaves[..k]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6962 known-answer vectors (the Certificate Transparency test
    /// tree), pinning the exact derivation for cross-implementation
    /// compatibility: eight leaf inputs of varying length and the tree
    /// hashes over their prefixes.
    #[test]
    fn tree_hash_matches_rfc6962_vectors() {
        fn hex(s: &str) -> Vec<u8> {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        }
        let leaves: Vec<Vec<u8>> = [
            "",
            "00",
            "10",
            "2021",
            "3031",
            "40414243",
            "5051525354555657",
            "606162636465666768696a6b6c6d6e6f",
        ]
        .iter()
        .map(|s| hex(s))
        .collect();

        let empty: [Vec<u8>; 0] = [];
        assert_eq!(
            tree_hash(&empty).to_vec(),
            hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        assert_eq!(
            tree_hash(&leaves[..1]).to_vec(),
            hex("6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d")
        );
        assert_eq!(
            tree_hash(&leaves[..8]).to_vec(),
            hex("5dc9da79a70659a9ad559cb701ded9a2ab9d823aad2f4960cfe370eff4604328")
        );
    }

    /// Strictly ascending 32-byte leaves for a set of the given size.
    fn set_of(n: u8) -> StatementSet {
        let hashes: Vec<[u8; 32]> = (0..n).map(|i| [i; 32]).collect();
        StatementSet::from_hashes(hashes).unwrap()
    }

    #[test]
    fn every_member_proves_and_verifies() {
        // Cover a power of two, one above, one below, and small edges.
        for size in [1u8, 2, 3, 7, 8, 9, 33] {
            let set = set_of(size);
            for i in 0..size {
                let hash = [i; 32];
                let proof = set.prove(&hash).unwrap();
                assert_eq!(proof.tree_size, size as u64);
                assert!(
                    verify_inclusion(&set.digest(), &hash, &proof),
                    "size {size} leaf {i}"
                );
                // The proof binds the exact statement hash, nothing else.
                assert!(!verify_inclusion(&set.digest(), &[0xEE; 32], &proof));
                // And the exact digest.
                assert!(!verify_inclusion(&[0xEE; 32], &hash, &proof));
            }
            assert!(set.prove(&[0xEE; 32]).is_none());
        }
    }

    #[test]
    fn tampered_proofs_fail() {
        let set = set_of(9);
        let hash = [4; 32];
        let good = set.prove(&hash).unwrap();

        let mut wrong_node = good.clone();
        wrong_node.path[0][0] ^= 1;
        assert!(!verify_inclusion(&set.digest(), &hash, &wrong_node));

        let mut wrong_index = good.clone();
        wrong_index.leaf_index = 5;
        assert!(!verify_inclusion(&set.digest(), &hash, &wrong_index));

        let mut truncated = good.clone();
        truncated.path.pop();
        assert!(!verify_inclusion(&set.digest(), &hash, &truncated));

        let mut extended = good.clone();
        extended.path.push([9; 32]);
        assert!(!verify_inclusion(&set.digest(), &hash, &extended));

        let mut wrong_size = good.clone();
        wrong_size.tree_size = 8;
        assert!(!verify_inclusion(&set.digest(), &hash, &wrong_size));
    }

    #[test]
    fn proof_wire_round_trip() {
        let set = set_of(9);
        let proof = set.prove(&[4; 32]).unwrap();
        let reparsed = InclusionProof::from_bytes(&proof.to_bytes()).unwrap();
        assert_eq!(reparsed, proof);

        assert_eq!(
            InclusionProof::from_bytes(&[0; 8]),
            Err(StatementSetError::ProofMalformed("header truncated"))
        );
        assert_eq!(
            InclusionProof::from_bytes(&[0; 17]),
            Err(StatementSetError::ProofMalformed(
                "path length not a multiple of 32"
            )),
        );
        // leaf_index (0) >= tree_size (0).
        assert_eq!(
            InclusionProof::from_bytes(&[0; 16]),
            Err(StatementSetError::ProofMalformed("leaf index out of range"))
        );
    }
}
