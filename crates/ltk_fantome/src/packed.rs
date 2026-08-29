//! [`PackedWadSource`]: read a packed WAD where the archive keeps it.
//!
//! A packed WAD is one archive entry holding a whole WAD. Reading a few chunks
//! out of one needs none of the entry's other bytes: the WAD's TOC names each
//! chunk's offset, so a source that can seek reaches them directly. A stored
//! entry allows that and a deflated one does not - deflate has no random
//! access, so a deflated entry must be inflated whole before any of it can be
//! addressed at all.
//!
//! [`normalize_archive`](crate::normalize_archive) is what makes an archive's
//! packed WADs stored; this is what spends that. An archive nobody normalized
//! still reads, at the cost of holding its WAD in memory.

use std::fmt;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use zip::read::ZipFileSeek;
use zip::{CompressionMethod, ZipArchive};

use crate::error::FantomeExtractError;
use crate::reader::read_entry;

/// A packed WAD's bytes, addressed from the WAD's own first byte.
///
/// Mount it with [`Wad::mount`](ltk_wad::Wad::mount) - which is what
/// [`FantomeReader::mount_packed_wad`](crate::FantomeReader::mount_packed_wad)
/// does - and the WAD's offsets land on the WAD's own bytes, whatever the
/// archive around it holds.
///
/// [`is_in_place`](Self::is_in_place) reports which of the two ways it got
/// them, since that is the difference between reading a chunk and inflating a
/// gigabyte to reach it.
pub struct PackedWadSource<'a, R> {
    inner: Inner<'a, R>,
}

/// Where a mounted WAD's bytes come from.
///
/// The two arms are what makes the seam real: an archive this crate normalized
/// takes the first, and one shipped by another tool takes the second without
/// the caller writing a second code path.
enum Inner<'a, R> {
    /// The entry read where the archive stores it, seek by seek.
    ///
    /// Boxed because the entry reader carries a copy of the archive's record
    /// for it, which is several times the size of the other arm and would
    /// otherwise be what every mounted WAD costs to move.
    InPlace {
        entry: Box<ZipFileSeek<'a, R>>,
        /// Position within the WAD.
        ///
        /// Tracked here because [`ZipFileSeek`]'s own [`Seek`] answers with the
        /// offset reached in the outer archive rather than in the entry, which
        /// is not the number the [`Seek`] contract asks for. Every seek below
        /// is absolute for the same reason: a relative one would be resolved
        /// against a position this type never reads back.
        pos: u64,
        /// Length of the entry, which the entry reader clamps reads and seeks
        /// to.
        len: u64,
    },
    /// The entry inflated into memory, which a deflated archive costs in full
    /// however few of the WAD's chunks the caller goes on to read.
    Buffered(Cursor<Vec<u8>>),
}

impl<'a, R: Read + Seek> PackedWadSource<'a, R> {
    /// The bytes of the entry at `index`, read in place when it is stored.
    ///
    /// # Errors
    ///
    /// Fails when the entry's header cannot be read, or when a deflated entry
    /// cannot be inflated.
    pub(crate) fn at_index(
        archive: &'a mut ZipArchive<R>,
        index: usize,
    ) -> Result<Self, FantomeExtractError> {
        let entry = archive.by_index_raw(index)?;
        let stored = entry.compression() == CompressionMethod::Stored;
        // The entry reader clamps to the compressed size, so that is the length
        // the position arithmetic here has to agree with. For a stored entry it
        // is the uncompressed size too, unless the archive says otherwise - and
        // an archive that says otherwise is one to follow rather than correct.
        let len = entry.compressed_size();
        drop(entry);

        let inner = if stored {
            Inner::InPlace {
                entry: Box::new(archive.by_index_seek(index)?),
                pos: 0,
                len,
            }
        } else {
            Inner::Buffered(Cursor::new(read_entry(&mut archive.by_index(index)?)?))
        };

        Ok(Self { inner })
    }
}

impl<R> PackedWadSource<'_, R> {
    /// Whether the WAD is read where the archive stores it.
    ///
    /// `false` means the entry was deflated and had to be inflated into memory
    /// first, which is the cost [`normalize_archive`](crate::normalize_archive)
    /// exists to remove.
    pub fn is_in_place(&self) -> bool {
        matches!(self.inner, Inner::InPlace { .. })
    }

    /// Length of the WAD in bytes.
    fn len(&self) -> u64 {
        match &self.inner {
            Inner::InPlace { len, .. } => *len,
            Inner::Buffered(cursor) => cursor.get_ref().len() as u64,
        }
    }

    /// Position within the WAD.
    fn position(&self) -> u64 {
        match &self.inner {
            Inner::InPlace { pos, .. } => *pos,
            Inner::Buffered(cursor) => cursor.position(),
        }
    }
}

impl<R> fmt::Debug for PackedWadSource<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackedWadSource")
            .field("in_place", &self.is_in_place())
            .field("len", &self.len())
            .field("position", &self.position())
            .finish()
    }
}

impl<R: Read + Seek> Read for PackedWadSource<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            Inner::InPlace { entry, pos, .. } => {
                let read = entry.read(buf)?;
                *pos += read as u64;
                Ok(read)
            }
            Inner::Buffered(cursor) => cursor.read(buf),
        }
    }
}

impl<R: Read + Seek> Seek for PackedWadSource<'_, R> {
    /// Seek within the WAD, counting from its own first byte.
    ///
    /// A seek past the end lands on the end rather than past it, in both arms:
    /// the entry reader clamps, and the buffered arm is held to the same answer
    /// so that which arm a caller got cannot change what it reads back.
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.len().checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position().checked_add_signed(delta),
        }
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek to a negative or overflowing position",
            )
        })?
        .min(self.len());

        match &mut self.inner {
            Inner::InPlace { entry, pos, .. } => {
                entry.seek(SeekFrom::Start(target))?;
                *pos = target;
            }
            Inner::Buffered(cursor) => cursor.set_position(target),
        }

        Ok(target)
    }
}

#[cfg(test)]
mod tests;
