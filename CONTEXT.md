# League Mod Toolkit

Tools and libraries for building, packaging and distributing League of Legends mods. A mod is
authored as a **mod project** and shipped as a **package**; the words below are the ones the
crates, the configs and the wiki all use for the same things.

## Language

### Mod shape

**Mod project**:
A mod in its authored, unpacked form: a manifest at the root and content laid out per layer.
_Avoid_: workspace, mod folder, source mod

**Layer**:
A named, prioritized slice of a mod's content. Every project has `base`; higher priorities
override lower ones when an overlay is built.
_Avoid_: variant, overlay, patch

**WAD target**:
The `.wad.client` archive a piece of content belongs in, named by a directory inside a layer.
_Avoid_: WAD file, archive target

**Chunk**:
One file inside a WAD, addressed by the hash of its path rather than by the path.
_Avoid_: entry, asset, file

**Chunk path**:
A chunk's path *within* its WAD target. Excludes the layer and the `.wad.client` directory.
_Avoid_: asset path, relative path

**Container**:
Any of the three shapes a mod's names can travel in: a `.fantome` archive, a mod project
directory, or a `.modpkg` package. Used when a rule holds for all three.
_Avoid_: format, package type

### Names and hashing

**Embedded hashtable**:
A table of names a mod carries *inside itself*, covering the names that mod introduces. Travels
with the content it names.
_Avoid_: bundled hashtable, mod hashtable, local hashtable

**Community hashtable**:
The ambient CommunityDragon name lists a tool ships or downloads separately, covering names the
game introduces. Never embedded in a mod.
_Avoid_: global hashtable, CDragon table, the hashtables

**Hashtable file**:
The plain-text payload of one table: one name per line, printable ASCII, no hash column.
_Avoid_: hash file, names file, wordlist

**Manifest entry**:
The declaration that gives one hashtable file its meaning - where it lives, its category, its
algorithm, and its key width. A table a manifest does not declare does not exist.
_Avoid_: table header, table config, registration

**Category**:
The lookup domain a table's names belong to: `game`, `binentries` or `binhashes`. Fixes which
hash space a name is resolved in.
_Avoid_: kind, namespace, table type

**Canonical name**:
A name after ASCII-lowercasing - the exact bytes that get hashed. Two names differing only in
case share one canonical name.
_Avoid_: normalized name, lowercased path

**Display casing**:
The casing a name is written in - both in a hashtable file and in the path a package stores.
Carried for humans; never hashed.
_Avoid_: original casing, pretty name

**Stored path**:
A chunk path spelled as its author wrote it. What a package holds and what an extraction names
files by. Hashing lowercases it first, so casing never changes a chunk's identity.
_Avoid_: display path, real path, original path

**Key**:
A name's hash truncated to the manifest entry's declared width. Duplicates and collisions are
both judged on keys, never on full hashes.
_Avoid_: hash, truncated hash, id

**Duplicate**:
The same canonical name appearing twice in a category. A writer must not emit one; a reader
keeps the first.
_Avoid_: repeat, collision

**Collision**:
Two *different* canonical names sharing one key. A packing error, not a duplicate.
_Avoid_: clash, conflict, duplicate

**Trimmed table**:
A `game` table that deliberately omits names a reader can recover elsewhere - one the package
already stores as a chunk, or one the community hashtables hold. Only `game` is ever trimmed.
_Avoid_: partial table, minimal table, sparse table

**Hex name**:
A key rendered as lowercase zero-padded hex - the placeholder a chunk lands under when nothing
names it, and the on-disk marker that a name is missing.
_Avoid_: hash name, raw name, fallback name

### Library storage

**Projectify**:
Turning a mod held as an archive into an unpacked mod project directory. A mod can be stored either
way; projectifying changes the storage shape, never the mod's identity.
_Avoid_: unpack, extract, convert, explode

**Normalize**:
Rewriting a mod's container into its canonical stored form - packed WADs held seekable in place,
metadata correct - without changing the mod's identity. Runs at import, on copies the importer
owns, never on a file the user handed in.
_Avoid_: repack, optimize, convert, fix up

### Overlay building

**Pass-through**:
Copying a chunk's already-compressed bytes from a mod's container straight into an overlay WAD,
identity verified in flight, never decompressed. The fallback for content that cannot pass through
is to decompress and recompress it.
_Avoid_: raw copy, zero-copy, fast path

### Preserving names

**Harvest**:
Reading a mod's names out of the places they currently survive - its on-disk chunk paths and the
strings inside its own bins - so they can be written into an embedded hashtable.
_Avoid_: scan, extract, recover, generate

**Rewrite**:
Writing a mod's harvested names back into its own archive, every other entry untouched. Runs only
when the harvest found a name the archive does not already declare.
_Avoid_: repack, resave, update

**Preserve**:
The whole motion the manager calls at import: harvest a mod's names, then rewrite them into the
mod's container. Harvest is the reading half; rewrite is the writing half.
_Avoid_: fix, import-fix, process

**Exclusions**:
The names a harvest leaves out of a table because a reader can recover them without the mod's
help - in practice, the community hashtables. Handed in by the caller, never assumed.
_Avoid_: filter, skip list, blacklist

**Unharvestable name**:
A name that survives nowhere the harvest can read: its chunk is hex-named and no bin still spells
it out. Counted and reported, never guessed at.
_Avoid_: lost name, missing name, unknown name

**Harvest report**:
A preserve's account of itself: whether the mod was rewritten and how many names were
unharvestable. What separates a mod that harvested cleanly from one that did not.
_Avoid_: result, summary, stats

**Covered mod**:
A mod whose names the community hashtables and its own stored paths already fully account for.
Preserving one adds nothing, so its archive is never touched.
_Avoid_: clean mod, known mod, no-op mod

**Unrepresentable path**:
A real chunk path no filesystem can hold: over the length limit, a reserved device name, a
trailing dot or space, or a case-only clash with a sibling. Its name survives only in a table.
_Avoid_: invalid path, bad path, illegal path

**Escape hatch**:
The convention that pairs an unrepresentable path with a hex-named file at the WAD root, so the
content lands on disk and the name lands in the `game` table.
_Avoid_: fallback, workaround

**File property**:
A `.bin` property whose value is the xxh64 of a path - the same hash space chunks are addressed
in, which is why one `game` table answers both.
_Avoid_: path property, file ref

**Hash property**:
A `.bin` property whose value is the fnv1a_32 of a string. Recoverable only through a
`binhashes` table.
_Avoid_: string hash, hashed string
