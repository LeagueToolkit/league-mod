# ADR-0015: Schema trait

- **Status:** Accepted
- **Date:** 2026-09-16
- **Crates:** `ltk_game_data`, `ltk_overlay`
- **Related:** #190, #191, `docs/design/game-data.md` [section 3](../design/game-data.md#s3)
  and [section 6](../design/game-data.md#s6), ADR-0006

## Context and problem statement

A property edit carries a bare value. The build coerces it to the type the property has in
the installed patch: the type comes from the class schema, the meta class dump, and the base
bin supplies it where the schema says nothing. `ltk_meta` 0.8.2 has no class schema; its
`M = NoMeta` type parameter is a placeholder, and ADR-0006 keeps schemas out of `ltk_meta`.

LTK Manager owns a class schema: a `MetaSchema` fed from `meta-api.leaguetoolkit.dev` with a
compiled-in snapshot, revisions keyed on a game build, and a lookup `expected(class, field,
build)` that answers `None` where the schema says nothing. Its shape carries a kind, a map
key kind, and a container item kind. It carries no referenced class for a pointer or an
embed and no default values. The manager pins `ltk_meta` to a git revision of
`league-toolkit`; this workspace pins the crates.io release. The two agree on `ltk_hash`.

`ltk_game_data` applies edits without an installation. Its tests run with a hand-written
schema.

## Decision drivers

- The library stays testable with no dump and no network.
- The manager implements the seam over `MetaSchema` with no new dependency.
- The overlay receives the schema once per build.
- A pointer's referenced class is read from the base value, the one place it is recorded.

## Considered options

1. **A trait in `ltk_game_data`.** `Schema::expected(class, field) -> Option<Shape>` and
   `Schema::has_class(class) -> bool`; the consumer implements it; `NoSchema` is the
   implementation that says nothing.
2. **A published schema crate.** The manager's `meta_schema` module moves to a crate both
   repositories depend on; `apply` takes that crate's type.
3. **Schema as data.** The dump's JSON is parsed by `ltk_game_data` and passed to `apply` as
   a value.

## Decision

**The class schema enters through a `Schema` trait that `ltk_game_data` defines and the
consumer implements.** `docs/design/game-data.md` [section 3](../design/game-data.md#s3)
states the trait, `Shape`, and `NoSchema`; [section 6](../design/game-data.md#s6) states how
the overlay receives an implementation.

`apply` takes `&dyn Schema`. `expected` returning `None` means the schema says nothing; the
base value's shape is the fallback and a `SchemaFallback` diagnostic reports it. `has_class`
answers whether a pinned class name is one the schema knows. A `Shape` carries a kind, a map
key kind, and an item kind, and no referenced class; a struct's class is read from the base
value or from a `pointer` or `embed` pin.

## Consequences

- **Positive:** the library tests coercion with a five-line schema; the manager implements
  two methods over the `MetaSchema` it holds.
- **Positive:** the trait names exactly the two questions the coercion asks.
- **Negative:** the manager's `Shape` and the library's `Shape` are two types with the same
  three fields; the implementation copies field by field.
- **Negative:** the trait fixes the questions. A coercion that needs a default value or a
  pointer's referenced class from the schema is a trait change.
- **Revisit when:** `ltk_meta` carries a class schema of its own, or a second consumer of
  `apply` appears.

## Pros and cons of the options

### A trait in `ltk_game_data`

- Good: no dependency, testable in isolation, the manager adapts what it has.
- Bad: two `Shape` types; every new question is a trait change.

### A published schema crate

- Good: one `Shape`, one loader, one cache.
- Bad: a crate extracted from the manager, versioned and released before the binding lands;
  the cache and network layers come with it.

### Schema as data

- Good: no trait; the dump format is documented.
- Bad: `ltk_game_data` parses a JSON format it does not own, and the manager holds the same
  data twice.
