//! String-table override application.
//!
//! Mods declare per-layer, per-locale string overrides in their project metadata
//! ([`ModProjectLayer::string_overrides`](ltk_mod_project::ModProjectLayer)),
//! shaped `locale -> field key -> replacement`. At build time the overlay builder
//! merges them across all enabled mods and applies them on top of the game's
//! `data/menu/{locale}/lol.stringtable` chunk, producing a patched stringtable in
//! the corresponding `Localized/Global.{locale}.wad.client` overlay WAD.
//!
//! # Merge semantics
//!
//! String conflicts resolve exactly like chunk conflicts:
//!
//! - Across mods, the mod closer to the front of the enabled list wins.
//! - Within a mod, layers apply in ascending priority order (higher priority
//!   layers overwrite lower ones).
//! - Within a layer, the [`DEFAULT_LOCALE`] bucket applies before the
//!   locale-specific bucket, so a locale-specific entry beats `"default"` in the
//!   same layer — but never beats a higher-priority mod's `"default"` entry.
//!
//! # Key syntax
//!
//! Keys are stringtable field names (hashed by `ltk_rst` per the table's format).
//! A key of the form `{hex}` (1–16 hex digits, e.g. `{f772a83b33773223}`) is
//! treated as a pre-computed hash instead, for entries whose plaintext name is
//! unknown.

use crate::builder::{EnabledMod, OverrideMeta, OverrideSource};
use crate::error::{Error, Result};
use crate::game_index::GameIndex;
use camino::{Utf8Path, Utf8PathBuf};
use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use xxhash_rust::xxh3::xxh3_64;

/// Locale bucket name whose overrides apply to every target locale.
pub const DEFAULT_LOCALE: &str = "default";

/// Which locales string overrides are applied to during an overlay build.
///
/// Configure via [`OverlayBuilder::with_string_overrides`](crate::OverlayBuilder::with_string_overrides).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum StringOverrideMode {
    /// Skip string patching entirely.
    #[default]
    Disabled,
    /// Patch only these locales (e.g. `["en_us"]`; case-insensitive).
    Locales(Vec<String>),
    /// Patch every locale that has a `Global.{locale}.wad.client` in the game.
    AllInstalled,
}

impl StringOverrideMode {
    /// Resolve the mode into a sorted, deduplicated list of lowercase locales.
    ///
    /// Locales without a matching `Global.{locale}.wad.client` are kept here and
    /// skipped later with a warning when the WAD lookup fails, so a misconfigured
    /// locale is visible in logs rather than silently dropped.
    pub(crate) fn resolve_locales(&self, game_index: &GameIndex) -> Vec<String> {
        let mut locales = match self {
            StringOverrideMode::Disabled => Vec::new(),
            StringOverrideMode::Locales(locales) => locales
                .iter()
                .map(|l| l.to_ascii_lowercase())
                .collect::<Vec<_>>(),
            StringOverrideMode::AllInstalled => game_index
                .localized_global_wads()
                .into_iter()
                .map(|(locale, _)| locale)
                .collect(),
        };
        locales.sort_unstable();
        locales.dedup();
        locales
    }
}

/// The stringtable chunk path inside `Global.{locale}.wad.client`.
pub(crate) fn stringtable_chunk_path(locale: &str) -> String {
    format!("data/menu/{locale}/lol.stringtable")
}

/// The WAD chunk path hash of [`stringtable_chunk_path`] for `locale`.
pub(crate) fn stringtable_chunk_hash(locale: &str) -> u64 {
    ltk_modpkg::utils::hash_chunk_name(&ltk_modpkg::utils::normalize_chunk_path(
        &stringtable_chunk_path(locale),
    ))
}

/// Parse a `{hex}` raw-hash key (1–16 hex digits) into its `u64` hash.
fn parse_raw_hash_key(key: &str) -> Option<u64> {
    let inner = key.strip_prefix('{')?.strip_suffix('}')?;
    if inner.is_empty() || inner.len() > 16 || !inner.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(inner, 16).ok()
}

/// A planned stringtable patch for one locale, computed in pass 1.
///
/// Carries everything needed to lazily generate the patched chunk bytes in
/// pass 2 — the base table is only read (and the RST only parsed) when the
/// target WAD actually needs rebuilding.
pub(crate) struct StringPatchPlan {
    /// Lowercase locale, e.g. `"en_us"`.
    pub(crate) locale: String,
    /// Path hash of the `lol.stringtable` chunk for this locale.
    pub(crate) chunk_hash: u64,
    /// Game-relative path of `Global.{locale}.wad.client`.
    pub(crate) wad_rel_path: Utf8PathBuf,
    /// Merged `field key -> replacement` map (sorted for determinism).
    pub(crate) overrides: BTreeMap<String, String>,
    /// A mod-shipped whole-stringtable override to use as the base instead of
    /// the game's chunk, when an enabled mod ships one (its meta entry is
    /// displaced from `all_meta` by the synthetic string-patch entry).
    pub(crate) base: Option<OverrideMeta>,
}

impl StringPatchPlan {
    /// Deterministic fingerprint of this patch's inputs, used as the synthetic
    /// meta entry's `content_hash` so per-WAD fingerprints (and therefore
    /// incremental rebuilds) react to any change in the effective overrides or
    /// the base table.
    pub(crate) fn fingerprint(&self) -> u64 {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"ltk-string-patch-v1\0");
        buf.extend_from_slice(self.locale.as_bytes());
        buf.push(0);
        buf.extend_from_slice(
            &self
                .base
                .as_ref()
                .map(|b| b.content_hash)
                .unwrap_or(0)
                .to_le_bytes(),
        );
        for (key, value) in &self.overrides {
            buf.extend_from_slice(key.as_bytes());
            buf.push(0);
            buf.extend_from_slice(value.as_bytes());
            buf.push(0);
        }
        xxh3_64(&buf)
    }

    /// Parse `base_bytes` as an RST stringtable, apply this plan's overrides,
    /// and serialize the patched table.
    pub(crate) fn apply(&self, base_bytes: &[u8]) -> Result<Vec<u8>> {
        let mut table =
            ltk_rst::Stringtable::from_reader(&mut Cursor::new(base_bytes)).map_err(|e| {
                Error::Other(format!(
                    "Failed to parse stringtable for locale '{}': {}",
                    self.locale, e
                ))
            })?;

        tracing::debug!(
            "Applying {} string override(s) to '{}' stringtable ({} entries, format {:?})",
            self.overrides.len(),
            self.locale,
            table.len(),
            table.format()
        );

        for (key, value) in &self.overrides {
            match parse_raw_hash_key(key) {
                Some(hash) => table.insert(hash, value.clone()),
                None => table.insert_str(key, value.clone()),
            }
        }

        let mut out = Vec::with_capacity(base_bytes.len() + 1024);
        table.to_writer(&mut out).map_err(|e| {
            Error::Other(format!(
                "Failed to write patched stringtable for locale '{}': {}",
                self.locale, e
            ))
        })?;
        Ok(out)
    }

    /// The synthetic override meta entry representing this patch in `all_meta`.
    pub(crate) fn to_override_meta(&self) -> OverrideMeta {
        OverrideMeta {
            content_hash: self.fingerprint(),
            uncompressed_size: self.base.as_ref().map(|b| b.uncompressed_size).unwrap_or(0),
            source: OverrideSource::StringPatch {
                chunk_path: Utf8PathBuf::from(stringtable_chunk_path(&self.locale)),
            },
            fallback_wad: Some(self.wad_rel_path.clone()),
            linked_bins: Vec::new(),
        }
    }
}

/// Case-insensitive lookup of a locale bucket in a layer's override map.
fn bucket<'a>(
    string_overrides: &'a HashMap<String, HashMap<String, String>>,
    locale: &str,
) -> Option<&'a HashMap<String, String>> {
    string_overrides
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(locale))
        .map(|(_, map)| map)
}

/// Merge string overrides from `enabled_mods` into one effective
/// `locale -> field key -> replacement` map per target locale.
///
/// See the module docs for the conflict-resolution rules. Locales whose
/// effective map ends up empty are omitted.
pub(crate) fn collect_effective_overrides(
    enabled_mods: &mut [EnabledMod],
    target_locales: &[String],
) -> Result<HashMap<String, BTreeMap<String, String>>> {
    let mut per_locale: HashMap<String, BTreeMap<String, String>> = HashMap::new();
    if target_locales.is_empty() {
        return Ok(per_locale);
    }

    // Front of the list wins, so fold back-to-front and let later writes overwrite.
    for enabled_mod in enabled_mods.iter_mut().rev() {
        let project = enabled_mod.content.mod_project()?;
        let mut layers = project.layers;
        layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

        for layer in &layers {
            if layer.string_overrides.is_empty() || !enabled_mod.is_layer_active(&layer.name) {
                continue;
            }

            for locale in target_locales {
                let effective = per_locale.entry(locale.clone()).or_default();
                for bucket_name in [DEFAULT_LOCALE, locale.as_str()] {
                    let Some(map) = bucket(&layer.string_overrides, bucket_name) else {
                        continue;
                    };
                    for (key, value) in map {
                        if key.is_empty() {
                            tracing::warn!(
                                "Mod '{}' layer '{}' has an empty string-override key; skipping",
                                enabled_mod.id,
                                layer.name
                            );
                            continue;
                        }
                        effective.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    per_locale.retain(|_, map| !map.is_empty());
    Ok(per_locale)
}

/// Read and decompress a single chunk from a game WAD.
pub(crate) fn read_game_chunk(
    game_dir: &Utf8Path,
    wad_rel_path: &Utf8Path,
    chunk_hash: u64,
) -> Result<Vec<u8>> {
    let abs_path = game_dir.join(wad_rel_path);
    let file = std::fs::File::open(abs_path.as_std_path())?;
    let mut wad = ltk_wad::Wad::mount(file)?;
    let chunk = *wad.chunks().get(chunk_hash).ok_or_else(|| {
        Error::Other(format!(
            "Chunk {:016x} not found in game WAD '{}'",
            chunk_hash, wad_rel_path
        ))
    })?;
    Ok(wad.load_chunk_decompressed(&chunk)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ModContentProvider;
    use ltk_mod_project::{ModProject, ModProjectLayer};
    use std::collections::HashSet;

    fn table_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut table = ltk_rst::Stringtable::new();
        for (key, value) in entries {
            table.insert_str(*key, *value);
        }
        let mut out = Vec::new();
        table.to_writer(&mut out).unwrap();
        out
    }

    fn plan(overrides: &[(&str, &str)]) -> StringPatchPlan {
        StringPatchPlan {
            locale: "en_us".to_string(),
            chunk_hash: stringtable_chunk_hash("en_us"),
            wad_rel_path: Utf8PathBuf::from("DATA/FINAL/Localized/Global.en_US.wad.client"),
            overrides: overrides
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            base: None,
        }
    }

    struct StringsOnlyContent {
        layers: Vec<ModProjectLayer>,
    }

    impl ModContentProvider for StringsOnlyContent {
        fn mod_project(&mut self) -> crate::error::Result<ModProject> {
            Ok(ModProject {
                name: "strings-mod".to_string(),
                display_name: "Strings Mod".to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                authors: vec![],
                license: None,
                tags: vec![],
                champions: vec![],
                maps: vec![],
                transformers: vec![],
                layers: self.layers.clone(),
                thumbnail: None,
            })
        }

        fn list_layer_wads(&mut self, _layer: &str) -> crate::error::Result<Vec<String>> {
            Ok(vec![])
        }

        fn read_wad_overrides(
            &mut self,
            _layer: &str,
            _wad_name: &str,
        ) -> crate::error::Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
            Ok(vec![])
        }

        fn read_wad_override_file(
            &mut self,
            _layer: &str,
            _wad_name: &str,
            _rel_path: &Utf8Path,
        ) -> crate::error::Result<Vec<u8>> {
            Ok(vec![])
        }

        fn read_raw_override_file(
            &mut self,
            _rel_path: &Utf8Path,
        ) -> crate::error::Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    fn layer(name: &str, priority: i32, buckets: &[(&str, &[(&str, &str)])]) -> ModProjectLayer {
        ModProjectLayer {
            name: name.to_string(),
            display_name: None,
            priority,
            description: None,
            string_overrides: buckets
                .iter()
                .map(|(locale, entries)| {
                    (
                        locale.to_string(),
                        entries
                            .iter()
                            .map(|(k, v)| (k.to_string(), v.to_string()))
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    fn strings_mod(id: &str, layers: Vec<ModProjectLayer>) -> EnabledMod {
        EnabledMod {
            id: id.to_string(),
            content: Box::new(StringsOnlyContent { layers }),
            enabled_layers: None,
        }
    }

    #[test]
    fn parse_raw_hash_key_variants() {
        assert_eq!(
            parse_raw_hash_key("{f772a83b33773223}"),
            Some(0xf772a83b33773223)
        );
        assert_eq!(parse_raw_hash_key("{ABC}"), Some(0xabc));
        assert_eq!(parse_raw_hash_key("{}"), None);
        assert_eq!(parse_raw_hash_key("{0123456789abcdef0}"), None); // 17 digits
        assert_eq!(parse_raw_hash_key("{xyz}"), None);
        assert_eq!(parse_raw_hash_key("game_client_quit"), None);
        assert_eq!(parse_raw_hash_key("{abc"), None);
    }

    #[test]
    fn apply_overrides_named_and_hash_keys() {
        let base = table_bytes(&[("game_client_quit", "Quit"), ("untouched", "Original")]);

        // Full 64-bit (untruncated) XXH3 of the key — ltk_rst masks it to the
        // table's hash width on insert, so get_key("added_key") must find it.
        let full_hash = xxhash_rust::xxh3::xxh3_64("added_key".as_bytes());

        let patched = plan(&[
            ("game_client_quit", "Bye"),
            (&format!("{{{full_hash:016x}}}"), "Injected"),
        ])
        .apply(&base)
        .unwrap();

        let table = ltk_rst::Stringtable::from_reader(&mut Cursor::new(&patched[..])).unwrap();
        assert_eq!(table.get_key("game_client_quit"), Some("Bye"));
        assert_eq!(table.get_key("untouched"), Some("Original"));
        assert_eq!(table.get_key("added_key"), Some("Injected"));
    }

    #[test]
    fn apply_rejects_invalid_table() {
        assert!(plan(&[("k", "v")]).apply(b"not an rst file").is_err());
    }

    #[test]
    fn fingerprint_reacts_to_inputs() {
        let a = plan(&[("key", "value")]);
        let b = plan(&[("key", "value")]);
        assert_eq!(a.fingerprint(), b.fingerprint());

        let c = plan(&[("key", "other")]);
        assert_ne!(a.fingerprint(), c.fingerprint());

        let mut d = plan(&[("key", "value")]);
        d.base = Some(OverrideMeta {
            content_hash: 0x1234,
            uncompressed_size: 10,
            source: OverrideSource::Raw {
                mod_id: "m".to_string(),
                rel_path: Utf8PathBuf::from("data/menu/en_us/lol.stringtable"),
            },
            fallback_wad: None,
            linked_bins: Vec::new(),
        });
        assert_ne!(a.fingerprint(), d.fingerprint());
    }

    #[test]
    fn merge_mod_priority_dominates() {
        // mods[0] is highest priority; its "default" bucket must beat the
        // lower-priority mod's locale-specific bucket.
        let mut mods = vec![
            strings_mod(
                "front",
                vec![layer("base", 0, &[("default", &[("key", "front")])])],
            ),
            strings_mod(
                "back",
                vec![layer("base", 0, &[("en_us", &[("key", "back")])])],
            ),
        ];

        let effective = collect_effective_overrides(&mut mods, &["en_us".to_string()]).unwrap();
        assert_eq!(effective["en_us"]["key"], "front");
    }

    #[test]
    fn merge_locale_beats_default_within_layer() {
        let mut mods = vec![strings_mod(
            "m",
            vec![layer(
                "base",
                0,
                &[
                    ("default", &[("key", "default-value"), ("other", "kept")]),
                    ("en_us", &[("key", "locale-value")]),
                ],
            )],
        )];

        let effective = collect_effective_overrides(&mut mods, &["en_us".to_string()]).unwrap();
        assert_eq!(effective["en_us"]["key"], "locale-value");
        assert_eq!(effective["en_us"]["other"], "kept");
    }

    #[test]
    fn merge_layer_priority_within_mod() {
        let mut mods = vec![strings_mod(
            "m",
            vec![
                layer("base", 0, &[("default", &[("key", "base")])]),
                layer("extras", 10, &[("default", &[("key", "extras")])]),
            ],
        )];

        let effective = collect_effective_overrides(&mut mods, &["en_us".to_string()]).unwrap();
        assert_eq!(effective["en_us"]["key"], "extras");
    }

    #[test]
    fn merge_skips_disabled_layers_and_empty_keys() {
        let mut mods = vec![EnabledMod {
            id: "m".to_string(),
            content: Box::new(StringsOnlyContent {
                layers: vec![
                    layer(
                        "base",
                        0,
                        &[("default", &[("key", "base"), ("", "dropped")])],
                    ),
                    layer("extras", 10, &[("default", &[("key", "extras")])]),
                ],
            }),
            enabled_layers: Some(HashSet::new()), // only base stays active
        }];

        let effective = collect_effective_overrides(&mut mods, &["en_us".to_string()]).unwrap();
        assert_eq!(effective["en_us"]["key"], "base");
        assert!(!effective["en_us"].contains_key(""));
    }

    #[test]
    fn merge_bucket_keys_case_insensitive() {
        let mut mods = vec![strings_mod(
            "m",
            vec![layer("base", 0, &[("EN_US", &[("key", "value")])])],
        )];

        let effective = collect_effective_overrides(&mut mods, &["en_us".to_string()]).unwrap();
        assert_eq!(effective["en_us"]["key"], "value");
    }

    #[test]
    fn merge_omits_empty_locales_and_targets() {
        let mut mods = vec![strings_mod(
            "m",
            vec![layer("base", 0, &[("ko_kr", &[("key", "value")])])],
        )];

        let effective = collect_effective_overrides(&mut mods, &["en_us".to_string()]).unwrap();
        assert!(effective.is_empty());

        let none = collect_effective_overrides(&mut mods, &[]).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn resolve_locales_modes() {
        let mut wad_index: HashMap<String, Vec<Utf8PathBuf>> = HashMap::new();
        for name in [
            "global.en_us.wad.client",
            "global.ko_kr.wad.client",
            "global.wad.client",
            "aatrox.wad.client",
        ] {
            wad_index.insert(
                name.to_string(),
                vec![Utf8PathBuf::from(format!("DATA/FINAL/{name}"))],
            );
        }
        let game_index = GameIndex {
            wad_index,
            hash_index: HashMap::new(),
            game_fingerprint: 0,
            subchunktoc_blocked: HashSet::new(),
        };

        assert!(StringOverrideMode::Disabled
            .resolve_locales(&game_index)
            .is_empty());
        assert_eq!(
            StringOverrideMode::Locales(vec!["en_US".to_string(), "en_us".to_string()])
                .resolve_locales(&game_index),
            vec!["en_us".to_string()]
        );
        assert_eq!(
            StringOverrideMode::AllInstalled.resolve_locales(&game_index),
            vec!["en_us".to_string(), "ko_kr".to_string()]
        );
    }
}
