//! The resolver: a caller-supplied source of chunk paths for chunk hashes.

use ltk_wad::WadHash;

/// A source of chunk paths for chunk hashes.
///
/// The batch shape lets a disk-backed table answer one query per slice. The chunk index never
/// takes a resolver. The object build takes an optional one.
///
/// Every [`ltk_wad::PathResolver`] implements this trait. `ltk_hashtable::GameResolver` is the
/// implementation this workspace uses.
pub trait ResolveWadPath {
    /// Visits `(index, path)` for every hash in `hashes` the source names.
    ///
    /// `index` is the position of the hash in `hashes`. Hashes the source does not name are not
    /// visited.
    fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str));
}

impl<T: ltk_wad::PathResolver + ?Sized> ResolveWadPath for T {
    fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str)) {
        for (index, path) in self.resolve_all(hashes).into_iter().enumerate() {
            if let Some(path) = path {
                visit(index, &path);
            }
        }
    }
}
