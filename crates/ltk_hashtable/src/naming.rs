//! [`free_table_name`]: the conventional file name of a new table.

use crate::Category;

/// Returns the first conventional file name for a new `category` table that is not taken.
///
/// The candidates are `{category}.hashes.txt`, then `{category}2.hashes.txt`,
/// `{category}3.hashes.txt` and onward. A candidate is taken if `is_taken` returns `true` for
/// it. `is_taken` receives the bare file name, and a container joins it onto its own table
/// directory both inside `is_taken` and on the result.
pub fn free_table_name(category: &Category, mut is_taken: impl FnMut(&str) -> bool) -> String {
    std::iter::once(format!("{category}.hashes.txt"))
        .chain((2u64..).map(|n| format!("{category}{n}.hashes.txt")))
        .find(|candidate| !is_taken(candidate))
        .expect("the candidate sequence is unbounded")
}
