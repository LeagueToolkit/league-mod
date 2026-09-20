# Research: game data declarations - engine constraints and design verdict

Handoff spec for the league-mod game data declaration system: the `game_data` manifest, the
declaration engine (league-mod#225 and the wider standard on the LTK wiki,
`reference/mod-packages/game-data`), and the overlay build that materialises declarations. It
records what the League client does with the data the build emits, and the design rules that
follow. It stands on its own and cites only public sources, so it can be copied into league-mod
as is.

Date: 2026-09-16. Client facts come from the retail Windows client, 16.16.8042073 for the WAD
layer and a 16.14-era build for the bin cache, with shipped data re-checked on the installed
client on the date above. Addresses are omitted on purpose; they drift every patch.

Every engine claim carries a mark:

| mark | meaning |
|---|---|
| **[traced]** | read off the client's disassembly, call sites and argument values checked |
| **[attested]** | confirmed against shipped data on the installed client |
| **[inferred]** | consistent with the code, not executed or observed |
| **[design]** | a recommendation, not a finding |

---

## 1. Verdict

1. **Keep routing as it is.** A materialised target is a replacement of a Riot chunk and must
   land in every WAD that holds that chunk, with identical compressed bytes. Any other
   placement is a fatal error at mount (section 2.2). A shared always-mounted WAD is not an
   option for targets.
2. **A shared WAD is an option for mod-authored companion bins**, where it buys cross-mod
   deduplication and deterministic placement. The engine works either way. Treat it as a
   builder policy, not a manifest concept (section 3.2).
3. **Address targets by entry name, resolved through the game index at build time.** When the
   index reports several holder chunks, edit all of them or the one the code-formatted path
   loads, and say which in the report (section 3.3).
4. **Add a first-class `modes` binding** that lowers to an append on the mode entry's extra-bin
   list inside the map's own bin (section 3.4). It needs the entries binding. The links-only
   engine can approximate it through the mode-specific data bins, as a stopgap only (section 4).
5. **Queue-level gating is outside this system.** Ranked, draft, Clash and customs share one
   mode entry, and no bin load point keys on the queue id. The manifest never carries a queue
   (section 3.5).
6. **Materialise `PROP` only, and let the build be the only validator.** Every failure on this
   path is silent in the client (section 2.6). The build report has to cover them all.

---

## 2. Engine facts the design rests on

### 2.1 WAD lookup and mount order **[traced]**

- A chunk lookup walks the mounted WADs in mount order and returns the first WAD holding the
  path hash. Nothing sorts the list and nothing scores a WAD. A chunk is not tied to the WAD it
  shipped in.
- Mount order is the client state order. The bootstrap WAD is mounted, read for `Bootstrap.bin`
  and unmounted again before anything else mounts. The core group then mounts and stays for the
  life of the process, in this order: `Global.wad`, `Scripts.wad`, `UI.wad`,
  `Shaders/Shaders.wad`, `ShaderCache.dx11.wad` (only when the renderer id is 3), `DATA.wad`,
  `Localized/Global.<lang>.wad`, `UI.<lang>.wad`. The map group mounts when a game starts and
  unmounts at map unload: `Maps/Shipping/Common.wad`, `Common.<lang>.wad`, `Map<N>.wad`,
  `Map<N>.<lang>.wad`, then any WADs the map bin names in `Map.WadDependencies`. Champion
  WADs mount on demand at loading step 52 and at later spawns, after all of the above.
- The core list is hardcoded. There is no data-driven way to add a WAD to the always-mounted
  set. `Map.WadDependencies` is data-driven but per map, mounts last, and a missing WAD there
  raises the install-corrupt fatal and writes a `SOFT_REPAIR` file that makes the patcher
  repair the install on the next launch.

### 2.2 The cross-WAD consistency check **[traced]**

At the end of every WAD mount the client looks up each chunk of the newly mounted WAD in the
other mounted WADs. A same-hash chunk whose checksum differs aborts the process with the
install-corrupt fatal, code 3, `Inconsistent`. The checksum is over the **compressed** bytes.
The check skips WADs that are not yet mounted, but a lookup miss mounts every later WAD in
turn, so everything is mounted within the first few lookups. Not exploitable.

Consequence: a modified `skin0.bin` in a core WAD plus the untouched original in
`Teemo.wad.client` dies the moment the champion WAD mounts. Removing the chunk from the
champion WAD's overlay copy avoids the check but still rewrites that WAD, so it gains nothing.
This check is the reason ltk_overlay fans one override out to every WAD holding the hash with
byte-identical compressed data. Keep that.

### 2.3 Bin loading and entry resolution **[traced]**

- One parser, one cache. Every property bin goes through the same code, whether requested by
  a code-formatted path, a `File` property, a `linked` header entry or the mode list below.
- A `linked` string is a literal chunk path: lowercased, XXH64 seed 0, no prefix added, no
  extension added, no second attempt. A miss returns false and the dependency is dropped with
  no log line. The list is recursive. Dependencies are loaded as base files, never as patches.
- Serialized `Link` values resolve after load by a cache-wide sweep of every loaded bin, first
  hit wins, in load order. The dependency list does not scope this. An entry name that
  collides with a Riot name is therefore a race, not an override.
- The code-driven lookups are scoped to one container: skin properties, the per-skin resource
  resolver entry and the character record chain search only the bin the code itself loaded.
  An entry meant for those lookups has to be in that bin.
- A bin that parses to zero entries reports success, is destroyed instead of cached, and is
  re-parsed on every request.
- `Bootstrap.bin` is parsed while only the bootstrap WAD is mounted. Nothing linked from it
  can come from any other WAD. Every champion, skin, map and mode bin parses after the core
  group is mounted.

### 2.4 The mode gate: `GameModeMapData.AdditionalPropertyDataPaths` **[traced]** **[attested]**

- The class `GameModeMapData` is the per-(map, mode) root object. Its entries are named by a
  path the client formats in code, `Maps/Shipping/Map<N>/Modes/<MODE>`, which is the most
  stable identifier class the client has. A builder can synthesize it from a map id and a mode
  name without any hash list.
- `AdditionalPropertyDataPaths` is a `list[string]`. The load virtual bails unless the object
  is the live mode, then for each string formats `"%s.bin"` and loads it through the bin
  cache, in list order. The unload virtual releases the same paths. So the values are written
  **without** the `.bin` extension, and a chunk reachable through this gate must be named
  `<something>.bin`. This is the opposite convention from `links`, where the value is the
  literal chunk path.
- The entries live in the map's own bin, `data/maps/shipping/map<N>/map<N>.bin`, and each of
  those bins ships in exactly one WAD, `Maps/Shipping/Map<N>.wad.client`. Confirmed for Map11,
  Map12, Map30, Map453 and Map22 on the installed client.
- It fires in custom games too. The gate is whichever (map, mode) the server placed the game
  in, not a mutator and not a queue.

Shipped roster on the installed client, 29 entries:

| map | entries with the list | entries without the list |
|---|---|---|
| Map11 | CLASSIC, PRACTICETOOL, URF, SNOWURF, SWIFTPLAY, ULTBOOK, ONEFORALL, TUTORIAL, TUTORIAL_MODULE_1, TUTORIAL_MODULE_2, TUTORIAL_MODULE_3, RUBY, RUBY_TRIAL_1, RUBY_TRIAL_2, RUBY_TRIAL_3 | ARSR, ASSASSINATE, DOOMBOTSTEEMO, WASD |
| Map12 | ARAM, FIRSTBLOOD, KIWI, KIWI_JADE | TUTORIAL, KINGPORO |
| Map30 | CHERRY | |
| Map453 | JADE, BASELINESR | |
| Map22 | | TFT |

Shipped list values, which matter for the stopgap in section 4:

| mode entry | `AdditionalPropertyDataPaths` |
|---|---|
| Map11 CLASSIC, PRACTICETOOL | `Maps/ModeSpecificData/CLASSIC`, `Maps/ModeSpecificData/WASD` |
| Map11 ONEFORALL | `Maps/ModeSpecificData/CLASSIC`, `Maps/ModeSpecificData/CLASSIC/WASD` |
| Map11 URF, SNOWURF | `Maps/ModeSpecificData/URF` |
| Map11 SWIFTPLAY | `Maps/ModeSpecificData/SWIFTPLAY` |
| Map11 ULTBOOK | `Maps/ModeSpecificData/ULTBOOK`, `.../ULTBOOK_BT`, `.../CLASSIC/WASD` |
| Map11 RUBY and the three trials | `Maps/ModeSpecificData/CLASSIC/WASD`, `.../RUBY` |
| Map11 TUTORIAL and the three modules | `Maps/ModeSpecificData/WASD`, `.../CLASSIC/WASD` |
| Map12 ARAM | `Maps/ModeSpecificData/ARAM`, `.../ULTBOOK`, `.../WASD` |
| Map12 FIRSTBLOOD | `Maps/ModeSpecificData/FIRSTBLOOD`, `.../CLASSIC/WASD` |
| Map12 KIWI, KIWI_JADE | `.../ARAM`, `.../WASD`, `.../KIWI` or `.../KIWI_JADE`, `.../AugmentGroups`, `.../AugmentOperators`, `.../AbilityAugmentSpellModifiers` |
| Map30 CHERRY | `Maps/ModeSpecificData/CHERRY`, `.../WASD` |
| Map453 BASELINESR | `Maps/ModeSpecificData/CLASSIC`, `.../WASD` |
| Map453 JADE | `Maps/ModeSpecificData/JADE`, `.../CLASSIC/WASD` |

Modes rotate. `maps/modespecificdata/brawl.bin`, `nexusblitz.bin` and `strawberry/wasd.bin`
exist in the public hash list but have no mode entry in the installed map WADs today. The game
index, not this table, decides what exists for a given install.

### 2.5 The queue is not a data key **[traced]** **[attested]**

The game process receives the queue from the platform: the match descriptor carries
`gameQueueConfigId`, `queueId` and `queueTypeName` beside `gameMode` and `mapId`. No bin load
point keys on any of them. The only per-mode data key the client formats is the
`/Map<N>/Modes/<MODE>` entry name. Ranked Solo, Ranked Flex, Normal Draft, Clash and custom
games all run `Maps/Shipping/Map11/Modes/CLASSIC`.

### 2.6 Everything on this path fails silent **[traced]**

| failure | what the client does | what the user sees |
|---|---|---|
| `linked` chunk missing | dropped, load continues | nothing |
| property type differs from the registered type | property skipped, load reports success | nothing |
| `Link` target never resolves | field left null | nothing, until a null-deref far away |
| bin parses to zero entries | destroyed, not cached | nothing |
| mode-list chunk missing | not read; the same acquire path drops a missing `linked` entry silently | nothing **[inferred]** |
| entry name collides with a Riot name | first hit in load order wins | intermittent |

Nothing in the client reports a moved file, a retyped property or a stale name. The build is
the only place these can be caught.

---

## 3. Design rules

### 3.1 Routing of materialised targets **[design]**, from 2.1 and 2.2

- A target materialised from declarations routes exactly like a whole-file override today:
  to every game WAD holding the chunk, compressed once from the same uncompressed input so
  every copy has the same checksum. `OverrideMeta.fallback_wad` and the hash-index fan-out
  already do this. Do not add a "shared WAD" mode for targets.
- A target held by one WAD, such as `data/maps/shipping/map11/map11.bin`, needs no fan-out.
  A target held by several, such as `maps/modespecificdata/classic.bin` in Map11 and Map453,
  fans out. The index knows which.

### 3.2 Placement of companion bins **[design]**, from 2.1, 2.2 and 2.3

A companion bin is a chunk the mod authors under its own namespace, `mods/<modid>/...`,
reached through `links` or the mode gate. It has no Riot twin, so the consistency check never
sees it, and any WAD mounted when the referencing bin parses can hold it.

- Default placement can stay what it is: the WAD directory the mod put the file under, else
  the dominant WAD by overlap.
- Optional policy: route every companion bin of every enabled mod into one overlay copy of a
  core WAD. Benefits: two mods shipping the same companion path under different WAD
  directories today produce two copies, and if the bytes differ the game dies with the same
  `Inconsistent` fatal; one shared WAD turns that into a build-time conflict. Placement also
  becomes deterministic instead of a heuristic. Cost: that WAD's overlay copy is rebuilt
  whenever any mod's companion bins change. Pick the smallest always-mounted, non-localized,
  non-gated WAD.

| candidate | size on disk | note |
|---|---|---|
| `Scripts.wad.client` | 6 MB | smallest; no locale; no gate |
| `Shaders/Shaders.wad.client` | 35 MB | |
| `ShaderCache.dx11.wad.client` | 45 MB | mounted only when the renderer id is 3; unsuitable |
| `DATA.wad.client` | 56 MB | the mount loop also probes `.server` and `.mobile` variants |
| `UI.wad.client` | 443 MB | |
| `Global.wad.client` | 1 GB | |
| `Maps/Shipping/Common.wad.client` | 100 MB | map group; remounted per map; fine for per-game content |

- Never place a companion bin only in a localized WAD, and never rely on
  `Bootstrap.windows.wad` for anything except content linked from `Bootstrap.bin`.

### 3.3 Targeting by entry name against the game index **[design]**, from 2.3

The index of every bin object entry is assumed implemented in LTK Manager. The declaration
system should use it as follows.

- A declaration may name an entry instead of a chunk. The build resolves the name to its
  holder chunks and their WADs for the installed patch. Match by FNV-1a 32 of the lowercased
  name; keep the author's casing in the manifest and show the attested casing from the index
  in reports.
- **Multiple holders.** The index reports every chunk holding the entry. The build edits all of
  them, or the one the code-formatted path loads when the entry is one of the scoped lookups
  (a skin properties entry, a per-skin `Resources` entry, a character record). Editing one copy
  of a multi-holder entry that serialized links reach is a race in load order. The report names
  the holders edited and the ones left alone.
- **New entries.** A mod-added entry carries the `Mods/<modid>/` prefix. The build proves the
  name absent from the index before packing. A collision is a packing error unless the
  declaration says the override is intended.
- **Schema.** The index gives the entry's class. Types come from the meta class dump for the
  installed patch, base bin as fallback with a report, as the standard already says.
- Entry-name targeting is sugar over chunk targeting. The plan still has one synthetic override
  per holder chunk, so the rest of the build is unchanged.

### 3.4 The `modes` binding **[design]**, from 2.4

Purpose: load a companion bin only in the named modes, and unload it with the mode. Proposed
manifest form, in the layer manifest the links engine already reads:

```yaml
version: 1
modules:
  - modes:
      - Map12/ARAM
      - Map12/KIWI
    load:
      - mods/example/aram-announcer.bin
```

Semantics and lowering:

- `modes` is a list of `Map<N>/<MODE>` selectors, matched case-insensitively against the
  index's `Maps/Shipping/Map<N>/Modes/<MODE>` entries. A selector with no entry in the
  installed game is a report-and-skip, because modes rotate. `Map<N>/*` may be allowed as
  "every mode entry on that map"; a bare `*` should not be, since a mod that wants every mode
  should use `links` on a permanent anchor instead.
- `load` is a list of literal chunk paths, written the way `links` values are, extension
  included. Each must end in `.bin`, because the client appends `.bin` unconditionally; an
  extensionless chunk is unreachable through this gate and is a packing error. Each must
  exist in enabled content once the overlay is in place, checked by the same pre-flight that
  checks `links`.
- The build lowers one module into one entries edit per selected mode entry, on the target
  `data/maps/shipping/map<N>/map<N>.bin`:

```yaml
  - target: data/maps/shipping/map12/map12.bin
    Maps/Shipping/Map12/Modes/ARAM:
      +AdditionalPropertyDataPaths:
        - mods/example/aram-announcer
```

  The value is the `load` path with `.bin` stripped. The target is derived, never authored,
  so a Riot repath of the map bin is caught by the index rather than by the mod.
- Entries that ship without the list (section 2.4 table, right column) need the property
  created. The entries engine should treat `+` on an absent list-typed property as create and
  add, with the element type taken from the schema. If that is not wanted, the lowering emits
  an unsigned set for those entries instead. Decide once and document it.
- Precedence and conflicts follow the standard: mod order, layer priority, module order. Two
  mods appending to the same mode entry compose; the appended values keep mod order. Duplicate
  values are dropped on the lowercased form.
- Ordering inside the list: Riot's values first, appended values after, in declaration order.
  The client loads them in that order.
- Do not also `links` the same companion bin from a permanent anchor such as `champions.bin`.
  The permanent reference keeps the refcount alive and defeats the unload.
- This lowering is an entries edit, so it lands with the entries binding, roadmap phase 2. It
  needs no new record shape: it is a list append and, for the no-list entries, a list set.

Interface to the manager: the mode roster comes from the index. A user-level mode filter is a
build input that intersects every module's `modes` list; toggling it changes only the map bins,
so one overlay serves every mode and the gate costs nothing at launch. The filter governs only
content that flows through the gate. Whole-file overrides and materialised targets in champion
WADs apply in every mode regardless, because nothing in the file layer is mode-aware.

### 3.5 What the manifest must not try to express **[design]**, from 2.5

- No queue selectors. "Not in ranked" cannot be a data gate; it is a launch-time decision for
  the manager, which knows the queue from the LCU gameflow session before the game process
  starts and can choose which profile to serve. The declaration system should not grow a
  `queues` key that it cannot honour.
- No gameplay promises. The gate is client-only. Anything gameplay-relevant is
  server-authoritative, so every mod on this path is about what the local client loads.

### 3.6 Things the build must never do **[design]**, from 2.2, 2.3 and the standard

- Never emit a `PTCH` chunk into the overlay. Every declaration materialises into a `PROP`.
- Never use `linked` as a scoping mechanism. It loads unconditionally with its parent.
- Never gate on `Maps/ModeSpecificData/WASD` or `.../CLASSIC/WASD`; nearly every mode loads
  them.
- Never rely on mount order, on a backslash-spelled dependency, or on any other cache-key
  trick to shadow or namespace. Mount order shadowing of a differing chunk is the fatal in 2.2;
  the backslash split duplicates a parse and makes link binding load-order dependent.
- Never ship a companion bin that names a Riot chunk path or `File` hash directly. Riot names
  belong in the generated binding the build materialises against the installed patch.

### 3.7 The build is the only validator **[design]**, from 2.6

Every check below is a report line in `overlay.json`, report-and-skip, never a build failure:

- every `links` and `load` path exists in the mounted set once the overlay is in place;
- every targeted entry exists in the resolved holder, with the class the schema expects;
- every edited property exists on that class with the expected type tag; list-valued
  properties first, because a stale list edit does not no-op, it empties the list;
- every mode selector resolves to a shipped entry;
- every mod-added entry name is absent from the game index, or the override is declared;
- every companion path sits under the mod namespace;
- a companion bin declared under both `load` and a permanent `links` anchor is flagged.

---

## 4. Mode scoping with the links-only engine, as a stopgap

The engine shipped with league-mod#225 supports `links`, `+links` and `-links` only. Until the
entries binding lands, a mod can approximate a mode gate by linking its companion bin from a
mode-specific data bin that the mode already loads. The mapping today, inverted from the
section 2.4 table:

| link target | loaded by |
|---|---|
| `maps/modespecificdata/classic.bin` | Map11 CLASSIC, PRACTICETOOL, ONEFORALL; Map453 BASELINESR |
| `maps/modespecificdata/urf.bin` | Map11 URF, SNOWURF |
| `maps/modespecificdata/swiftplay.bin` | Map11 SWIFTPLAY |
| `maps/modespecificdata/ultbook.bin` | Map11 ULTBOOK; **Map12 ARAM** |
| `maps/modespecificdata/ultbook_bt.bin` | Map11 ULTBOOK |
| `maps/modespecificdata/ruby.bin` | Map11 RUBY, RUBY_TRIAL_1, RUBY_TRIAL_2, RUBY_TRIAL_3 |
| `maps/modespecificdata/aram.bin` | Map12 ARAM, KIWI, KIWI_JADE |
| `maps/modespecificdata/firstblood.bin` | Map12 FIRSTBLOOD |
| `maps/modespecificdata/kiwi.bin`, `kiwi_jade.bin` | Map12 KIWI, KIWI_JADE respectively |
| `maps/modespecificdata/cherry.bin` | Map30 CHERRY |
| `maps/modespecificdata/jade.bin` | Map453 JADE |
| `maps/modespecificdata/wasd.bin`, `classic/wasd.bin` | nearly everything; never a gate |

Limits that make this a stopgap and not the design:

- It scopes to "every mode that loads bin X", not to one mode. ARAM alone is unreachable,
  since `aram.bin` is also loaded by KIWI and KIWI_JADE, and `ultbook.bin` is loaded by ARAM.
- Several of these bins ship in more than one map WAD (`classic.bin` in Map11 and Map453,
  `wasd.bin` in four), so the edit fans out.
- Whether a `linked` dependency is released when its parent is released on mode unload is
  untraced. Assume the companion bin may stay resident for the process.

---

## 5. Unverified items, and how to observe each

None of these changes the rules above. Each should be observed in a real game before a
release relies on it, because there is no log line for any of them.

1. **Missing mode-list chunk.** Declare a `load` path that does not exist, start a custom
   game in that mode. Expected: the game loads normally. A fatal or a hang would mean the
   loader asserts where `linked` does not, and the pre-flight becomes a hard requirement.
2. **Link visibility into mode bins.** The mode's own objects resolve their serialized links
   when the map bin parses, before the mode list loads. Expected: a Riot object cannot point at
   an entry in a companion bin through this gate; the other direction works, and so do runtime
   by-name lookups such as character records and resource resolvers, which sweep every loaded
   bin at use time **[inferred]**. Observe with a resolver entry the mode bin adds and a spell
   that asks for that key.
3. **Timing against champion bins.** The mode list loads at map load, before champion WADs
   mount at loading step 52, so entries in a companion bin should be visible to champion and
   skin bins parsed later **[inferred]**. Observe with a skin resolver repointed to an effect
   defined in the companion bin.
4. **Release on mode unload.** Confirm the companion bin's objects are gone in the next game
   of a different mode, for example by a visible effect that must not appear there.

---

## 6. Worked example

An ARAM-only announcer pack that adds its own event objects and repoints nothing Riot-owned:

```
my-mod/
|-- mod.config.toml
|-- content/
|   |-- base/
|   |   |-- game_data.yaml
|   |   |-- Map12.wad.client/
|   |   |   |-- mods/aram-announcer/announcer.bin      # entries named Mods/aram-announcer/...
```

```yaml
# content/base/game_data.yaml
version: 1
modules:
  - modes: [Map12/ARAM]
    load: [mods/aram-announcer/announcer.bin]
```

What the build emits: one synthetic override for `data/maps/shipping/map12/map12.bin`, routed
to `Maps/Shipping/Map12.wad.client`, whose `Maps/Shipping/Map12/Modes/ARAM` entry now lists
`mods/aram-announcer/announcer` after Riot's three values; and the companion chunk, routed to
the declared WAD or to the shared companion WAD under policy 3.2. In KIWI, on Summoner's Rift
and in every other mode the companion bin is never requested. In ARAM it loads at map load and
is released at map unload.

---

## 7. Sources

- LTK wiki, `making-mods/game-data` (the shipped links engine) and
  `reference/mod-packages/game-data` (the wider standard).
- league-mod issues #190, #191 and #225, and `docs/research/issue-190-declaring-bin-patches.md`
  in that repository, whose section 2.9.3 table row H10 is the mode gate this spec builds on.
- `ltk_overlay` in league-mod: `wad_builder.rs` (compressed-checksum note), `game_index.rs`
  and `builder/metadata.rs` (hash-index fan-out and WAD routing).
- Shipped data: the installed retail client on 2026-09-16, scanned with a bin object grep over
  the map WADs, and the CommunityDragon `hashes.game.txt` and `hashes.binentries.txt` lists.
