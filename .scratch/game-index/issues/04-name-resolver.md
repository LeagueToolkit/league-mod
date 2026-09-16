---
title: "Game index: name resolver boundary"
labels: area:api
---

Part of the game index map.
Type: grilling
Status: resolved
Blocked by: 02

## Question

Which shape does name resolution take at the crate boundary: a trait the crate defines
(`resolve(&self, hash: WadHash) -> Option<&str>` and the bin-hash counterpart), implemented by
`ltk_hashtable` here and by the manager's `LayeredHashDb` there, or a direct dependency on
`ltk_hashtable`? Which operations of the crate need names at all, given there is no tree and no
search: candidates are naming a chunk in a diagnostic and telling a `.bin` chunk from one to sniff
during the object index build. State the churn each option causes in the manager's call sites
from the consumer research.

## Answer

- Name resolution enters through a trait `ltk_game_index` defines, batch-shaped:
  `fn for_each_named(&self, hashes: &[WadHash], visit: &mut dyn FnMut(usize, &str))`.
- An optional `hashtable` feature implements the trait for `ltk_hashtable::GameResolver`. The
  manager implements it over its layered hash db.
- The trait sits at the crate root. The chunk index takes no resolver anywhere; rows carry no
  path strings. The object build is the only consumer, for sorting chunks into named `.bin`,
  bare-named and unnamed-to-sniff.
- The object index is hash-only. Object and class display names stay in the manager, and the
  manager's `ObjectNames` trait stays there; only its WAD-path half becomes the crate's trait.
- The resolver is optional at build time. Absent, every chunk is sniffed by magic.
