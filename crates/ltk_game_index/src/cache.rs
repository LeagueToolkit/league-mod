//! The on-disk form of an index: a MessagePack document with a version and a fingerprint.
//!
//! A cache file holds three consecutive MessagePack values: the format version, the
//! fingerprint, and the body. The version is read first. A foreign version is reported
//! without decoding the rest.

use std::fs::File;
use std::io::{BufReader, BufWriter, Write as _};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Serialize, de::DeserializeOwned};

use crate::{BuildError, Fingerprint};

/// A cache that did not load or save.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// The file did not read.
    #[error("cannot read cache {path}")]
    Read {
        /// The cache file.
        path: Utf8PathBuf,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
    /// The file did not write.
    #[error("cannot write cache {path}")]
    Write {
        /// The cache file.
        path: Utf8PathBuf,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
    /// The bytes are not an index.
    #[error("cache {path} is not a valid index")]
    Decode {
        /// The cache file.
        path: Utf8PathBuf,
        /// The failure.
        #[source]
        source: rmp_serde::decode::Error,
    },
    /// The index did not encode.
    #[error("cache {path} cannot be encoded")]
    Encode {
        /// The cache file.
        path: Utf8PathBuf,
        /// The failure.
        #[source]
        source: rmp_serde::encode::Error,
    },
    /// The file carries a format version other than the crate's.
    #[error("cache {path} has format version {found}, expected {expected}")]
    Version {
        /// The cache file.
        path: Utf8PathBuf,
        /// The version in the file.
        found: u32,
        /// The version the crate reads.
        expected: u32,
    },
    /// The file was built for another installation state.
    #[error("cache {path} was built for fingerprint {cached}, installation is {current}")]
    Stale {
        /// The cache file.
        path: Utf8PathBuf,
        /// The fingerprint in the file.
        cached: Fingerprint,
        /// The fingerprint of the installation.
        current: Fingerprint,
    },
    /// The installation's fingerprint did not compute.
    #[error(transparent)]
    Build(#[from] BuildError),
}

impl CacheError {
    /// Whether the error is a cache file that does not exist.
    #[must_use]
    pub fn is_missing_file(&self) -> bool {
        matches!(self, Self::Read { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
    }
}

/// A cache file's header and body.
#[derive(Debug)]
pub(crate) struct Document<T> {
    pub fingerprint: Fingerprint,
    pub body: T,
}

/// Reads the cache at `path`, whose format version is `expected`.
///
/// # Errors
///
/// [`CacheError::Read`], [`CacheError::Decode`] and [`CacheError::Version`].
pub(crate) fn read<T: DeserializeOwned>(
    path: &Utf8Path,
    expected: u32,
) -> Result<Document<T>, CacheError> {
    let file = File::open(path.as_std_path()).map_err(|source| CacheError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let decode = |source| CacheError::Decode {
        path: path.to_path_buf(),
        source,
    };

    let found: u32 = rmp_serde::from_read(&mut reader).map_err(decode)?;
    if found != expected {
        return Err(CacheError::Version {
            path: path.to_path_buf(),
            found,
            expected,
        });
    }
    let fingerprint: Fingerprint = rmp_serde::from_read(&mut reader).map_err(decode)?;
    let body: T = rmp_serde::from_read(&mut reader).map_err(decode)?;
    Ok(Document { fingerprint, body })
}

/// Writes `body` to `path` under `version` and `fingerprint`, atomically.
///
/// The bytes go to a sibling temporary file, which is renamed over `path`. A failure leaves
/// no partial file at `path`.
///
/// # Errors
///
/// [`CacheError::Write`] and [`CacheError::Encode`].
pub(crate) fn write<T: Serialize>(
    path: &Utf8Path,
    version: u32,
    fingerprint: Fingerprint,
    body: &T,
) -> Result<(), CacheError> {
    let write_error = |source| CacheError::Write {
        path: path.to_path_buf(),
        source,
    };
    let encode_error = |source| CacheError::Encode {
        path: path.to_path_buf(),
        source,
    };

    let mut bytes = Vec::new();
    rmp_serde::encode::write(&mut bytes, &version).map_err(encode_error)?;
    rmp_serde::encode::write(&mut bytes, &fingerprint).map_err(encode_error)?;
    rmp_serde::encode::write(&mut bytes, body).map_err(encode_error)?;

    let parent = path.parent().unwrap_or(Utf8Path::new("."));
    std::fs::create_dir_all(parent.as_std_path()).map_err(write_error)?;
    let temporary = temporary_sibling(path);
    let outcome = (|| {
        let mut writer = BufWriter::new(File::create(temporary.as_std_path())?);
        writer.write_all(&bytes)?;
        writer
            .into_inner()
            .map_err(std::io::Error::other)?
            .sync_all()?;
        std::fs::rename(temporary.as_std_path(), path.as_std_path())
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(temporary.as_std_path());
    }
    outcome.map_err(write_error)
}

/// A temporary file name next to `path`, unique to this process and moment.
fn temporary_sibling(path: &Utf8Path) -> Utf8PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let file_name = path.file_name().unwrap_or("cache");
    path.with_file_name(format!(".{file_name}.{}.{nanos}.tmp", std::process::id()))
}
