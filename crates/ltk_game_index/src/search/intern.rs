//! String interning in one buffer, keyed by the XXH3 hash of each string.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hasher};

/// The 64-bit hash an [`Interner`] keys a string by.
pub(super) fn hash(string: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(string.as_bytes())
}

/// Passes a key that is already a hash through as its own hash.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Identity(u64);

impl Hasher for Identity {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(byte);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

/// A map keyed by a well-distributed 64-bit hash.
pub(super) type HashKeyed<V> = HashMap<u64, V, BuildHasherDefault<Identity>>;

/// Distinct strings in first-seen order, each followed by `\n` in one buffer.
///
/// A string is found by its hash and confirmed against the buffer. Two strings sharing a hash
/// are told apart through a second map of the later ones.
#[derive(Debug, Default)]
pub(super) struct Interner {
    /// Every string, each followed by `\n`.
    text: String,
    /// The byte offset of each string.
    starts: Vec<u32>,
    /// The hash of each string.
    hashes: Vec<u64>,
    /// The first string of each hash.
    by_hash: HashKeyed<u32>,
    /// Strings whose hash an earlier string holds.
    collided: HashMap<Box<str>, u32>,
}

impl Interner {
    /// The id of `string`, whose hash is `hash`, interned when new. `string` holds no `\n`.
    pub fn intern(&mut self, string: &str, hash: u64) -> u32 {
        match self.by_hash.entry(hash) {
            Entry::Vacant(slot) => {
                let id = push(
                    &mut self.text,
                    &mut self.starts,
                    &mut self.hashes,
                    string,
                    hash,
                );
                slot.insert(id);
                id
            }
            Entry::Occupied(slot) => {
                let first = *slot.get();
                if get(&self.text, &self.starts, first) == string {
                    return first;
                }
                if let Some(&id) = self.collided.get(string) {
                    return id;
                }
                let id = push(
                    &mut self.text,
                    &mut self.starts,
                    &mut self.hashes,
                    string,
                    hash,
                );
                self.collided.insert(string.into(), id);
                id
            }
        }
    }

    /// The number of strings.
    pub fn len(&self) -> usize {
        self.starts.len()
    }

    /// The string `id`.
    pub fn get(&self, id: u32) -> &str {
        get(&self.text, &self.starts, id)
    }

    /// The hash of string `id`.
    pub fn hash_of(&self, id: u32) -> u64 {
        self.hashes[id as usize]
    }

    /// Drops every string from `len` on.
    pub fn truncate(&mut self, len: usize) {
        if len >= self.len() {
            return;
        }
        for id in len..self.len() {
            let id = id as u32;
            let hash = self.hashes[id as usize];
            if self.by_hash.get(&hash) == Some(&id) {
                self.by_hash.remove(&hash);
            } else {
                let string: Box<str> = self.get(id).into();
                self.collided.remove(&string);
            }
        }
        self.text.truncate(self.starts[len] as usize);
        self.starts.truncate(len);
        self.hashes.truncate(len);
    }

    /// The buffer, every string followed by `\n`, and the offset of each string.
    pub fn into_parts(self) -> (String, Vec<u32>) {
        (self.text, self.starts)
    }
}

fn push(
    text: &mut String,
    starts: &mut Vec<u32>,
    hashes: &mut Vec<u64>,
    string: &str,
    hash: u64,
) -> u32 {
    let id = starts.len() as u32;
    starts.push(text.len() as u32);
    text.push_str(string);
    text.push('\n');
    hashes.push(hash);
    id
}

fn get<'a>(text: &'a str, starts: &[u32], id: u32) -> &'a str {
    let start = starts[id as usize] as usize;
    let end = starts
        .get(id as usize + 1)
        .map_or(text.len(), |&next| next as usize);
    &text[start..end - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_interns_once_and_a_shared_hash_keeps_both_strings() {
        let mut interner = Interner::default();
        assert_eq!(interner.intern("a", hash("a")), 0);
        assert_eq!(interner.intern("b", hash("b")), 1);
        assert_eq!(interner.intern("a", hash("a")), 0);
        assert_eq!(interner.intern("c", hash("a")), 2);
        assert_eq!(interner.intern("c", hash("a")), 2);
        assert_eq!(interner.get(2), "c");
        assert_eq!(interner.len(), 3);

        interner.truncate(1);
        assert_eq!(interner.len(), 1);
        assert_eq!(interner.intern("c", hash("a")), 1);
        assert_eq!(interner.intern("b", hash("b")), 2);
        assert_eq!(interner.intern("a", hash("a")), 0);
        let (text, starts) = interner.into_parts();
        assert_eq!(text, "a\nc\nb\n");
        assert_eq!(starts, [0, 2, 4]);
    }
}
