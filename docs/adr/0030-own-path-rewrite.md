# ADR-0030: Own-path rewrite

- **Status:** Accepted
- **Date:** 2026-09-23
- **Crates:** `ltk_game_data`
- **Related:** ADR-0029, `docs/design/game-data.md` [section 6](../design/game-data.md#s6)

## Context and problem statement

The game-data reference states that a clone rewrites the copy's own-path property where its
class carries one, and names no property. The class data of patch 16.18 (build 8175716) holds
`objectPath` as a `hash` on 12 classes, among them `VfxSystemDefinitionData`,
`SkinCharacterDataProperties` and `SpellObject`, and as a `string` on `CustomShaderDef`.
`VfxSystemDefinitionData` also carries `particlePath`, a `string`, and `ContextualActionData`
carries `mObjectPath`, a `string`. The class data names fields and types, not which value a
field holds.

A clone that keeps its source's own path names the source.

## Decision drivers

- A clone names itself wherever its source named itself.
- No list of classes or fields to keep in step with the game.
- No property a clone does not own is changed.

## Considered options

1. **By value.** A top-level `hash` property equal to the source's object hash, and a
   top-level `string` property equal to the source's path, name the clone.
2. **By field.** The fields `objectPath`, `particlePath` and `mObjectPath` name the clone,
   whatever they hold.
3. **No rewrite.** A clone is a copy; an author sets the own path in `set`.

## Decision

**A clone rewrites every top-level `hash` property holding its source's object hash and every
top-level `string` property spelling its source's path, compared ASCII case-insensitively.**
`docs/design/game-data.md` [section 6](../design/game-data.md#s6) states the rule. A string is
left as it is where either name is hash-form.

## Consequences

- **Positive:** a class gaining or renaming an own-path field needs no change here.
- **Positive:** a field holding another object's path is copied as it is.
- **Negative:** a top-level `hash` or `string` property naming the source for a reason other
  than identity is rewritten too. No such field is known.
- **Negative:** an own path held below the top level, or as a `link`, is not rewritten.
- **Revisit when:** a class holds its own path in a nested struct or a `link`.

## Pros and cons of the options

### By value

- Good: schema-free, and matches every own-path field of the class data.
- Bad: rewrites by coincidence of value, not by the field's meaning.

### By field

- Good: rewrites exactly the fields named.
- Bad: a list tracking Riot's field names, and a field holding a stale path is rewritten too.

### No rewrite

- Good: a clone is a byte copy of its source's values.
- Bad: the reference's rule is not met, and every clone of a particle needs the same `set` key.
