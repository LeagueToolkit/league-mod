# ADR-0021: Game-copy references

- **Status:** Proposed
- **Date:** 2026-09-19
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** ADR-0009, ADR-0016, ADR-0019, ADR-0020, LTK Manager ADR-0042,
  `docs/design/game-data.md` [section 4](../design/game-data.md#s4) and
  [section 6](../design/game-data.md#s6)

## Context and problem statement

A property edit carries a literal (ADR-0016). A literal holds the value of the day it was written.
A skin swap declares the resolver map of another skin into a base skin:

```yaml
- entries:
    Characters/Teemo/Skins/Skin0/Resources:
      +resourceMap:
        Jade_Teemo_BA_cas: Characters/Jade_Teemo/Skins/Skin0/Particles/Jade_Teemo_Base_BA_cas
        # ... every key of the Jade Teemo map, copied by hand
```

Riot edits the Jade Teemo map in a later patch. The mod keeps the copied keys and misses the
change. The author intent is "Riot's Jade Teemo map, at every build".

The overlay holds the object index and reads a game chunk by hash (ADR-0009); an `entries` module
resolves an entry to its declaring chunks through it ([section 6](../design/game-data.md#s6)).
`ltk_game_data` has no dependency on `ltk_game_index` (rule D8) and reads override bytes through
a callback ([section 3](../design/game-data.md#s3)).

A one-key mapping keyed by a type name is a pin (ADR-0019). No Riot field hashes to `ref`
(`0x42f48402`) in the meta database through 16.18 (build 8175716, 5507 classes). The CDTB entry
name table (143588 names) holds no `:`. LTK Manager's Copy path writes `<entry>:<property path>`.

## Decision drivers

- A declaration names a value by where it lives, and the build reads it.
- One reading of the value form in YAML, TOML, and JSON.
- The same types and reports as a literal: a reference coerces by the property's shape.
- `ltk_game_data` stays free of the game index.
- The build result depends on the installed game and the enabled mods, and on nothing else.

## Considered options

1. **A reference value reading the installed game's copy.** `{ref: "<entry>:<path>"}`, YAML
   `!ref`, anywhere a value is. The caller supplies the entry's object; the game's copy answers.
2. **A reference value reading the build state.** The same form, resolved against the target
   state after lower-precedence mods and earlier modules.
3. **An object binding only.** `objects` with `clone` (the game-data reference roadmap) copies
   whole objects; no property-level reference.
4. **No standard change.** Authoring tools copy literals; a later patch needs a re-copy.

## Decision

**A reference is a value: a one-key mapping keyed `ref` whose value is `<entry>:<property
path>`, resolved against the installed game's copy of the entry.** A YAML `!ref` tag loads as the
same mapping. `apply()` reads the entry's object through a caller callback, resolves the path in
it, and checks the resolved value's shape against the property's shape. The overlay answers the
callback from the first declaring chunk of the entry in game-index archive order, before any mod
content applies. An unresolved reference skips its key with a reason code.
[Section 4](../design/game-data.md#s4) states the loading and
[section 6](../design/game-data.md#s6) the resolution.

## Consequences

- **Positive:** a declared copy of Riot data follows Riot's patches with no re-authoring.
- **Positive:** a reference sits anywhere a value does: a set, a `+` or `-` operand, a map value,
  a list item, a `set` field of a struct pin.
- **Positive:** a reference types exactly. The game value carries its own kinds; no literal
  spelling is involved.
- **Negative:** `ref` joins the reserved one-key names. A Riot field named `ref` takes the dotted
  spelling as the only key of a block.
- **Negative:** a build with a reference builds or loads the object index (rule D10 widens).
- **Negative:** a reference cannot read a mod's content, another mod's edits included.
- **Negative:** an entry declared in several chunks with differing values resolves to the first
  holder's copy.
- **Revisit when:** a mod needs another mod's value, or Riot adds a field named `ref`.

## Pros and cons of the options

### A reference value reading the installed game's copy

- Good: the result is a function of the game and the declaration; mod order does not change it.
- Good: the overlay already resolves entries to chunks for `entries` modules.
- Bad: no reference to mod content.

### A reference value reading the build state

- Good: a mod can reference another mod's edits.
- Bad: the result depends on mod precedence. Two references between mods form a cycle. The target
  fingerprint spans every mod that edits the referenced entry.

### An object binding only

- Good: `objects` is on the roadmap and covers whole-object copies.
- Bad: a property-level copy is a whole-object clone plus a hand-kept list of the other fields.

### No standard change

- Good: nothing to specify or build.
- Bad: every Riot patch that touches a copied value needs the author to copy it again.
