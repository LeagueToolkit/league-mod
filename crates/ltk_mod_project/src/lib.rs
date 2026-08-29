use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;

mod cancellation;
mod config_format;
pub mod error;
pub mod import;
mod license_file;
mod modignore;
pub mod pack;
mod package_format;

#[cfg(any(feature = "fantome", feature = "modpkg"))]
mod hashtable_routes;

#[cfg(feature = "fantome")]
pub mod fantome;
#[cfg(feature = "modpkg")]
pub mod modpkg;
#[cfg(feature = "fantome")]
pub mod preserve;

pub use cancellation::Cancellation;
pub use config_format::ConfigFormat;
pub use error::{ModProjectError, SerializeError};
#[cfg(any(feature = "fantome", feature = "modpkg"))]
pub use hashtable_routes::DuplicateHashtableName;
pub use import::{
    ConfigRefusal, ImportError, ImportFormat, ImportProgress, ImportReporter, ImportStage,
    ImportTarget, NoConfig, ProjectImporter, ProjectPath, ProjectPaths,
};
pub use license_file::{canonical_license_file_name, find_license_file, LICENSE_FILE_NAMES};
pub use modignore::{
    ContentWalk, ContentWalkError, ModIgnore, ModIgnoreError, ModIgnoreMatch, ModIgnoreRule,
    MODIGNORE_FILE_NAME,
};
pub use pack::{
    IgnoreMode, PackError, PackFormat, PackFormatReport, PackOptions, PackPlan, PackProgress,
    PackReport, PackReporter, PackStage, PlannedFile, PlannedHashtable, PlannedLayer,
    PlannedLicense, ProjectPacker,
};
pub use package_format::PackageFormat;
#[cfg(feature = "fantome")]
pub use preserve::{preserve_archive_names, HarvestReport, PreserveError, PreserveOutcome};

/// The directory every layer's content sits under, in the project root.
pub const CONTENT_DIR_NAME: &str = "content";

/// The directory a project's declared hashtable files sit under, in the
/// project root.
///
/// Deliberately outside [`CONTENT_DIR_NAME`]: a table is never a packing
/// candidate and never meets `.modignore`.
pub const HASHES_DIR_NAME: &str = "hashes";

/// Well-known mod tags for common mod categories.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum WellKnownModTag {
    LeagueOfLegends,
    Tft,
    ChampionSkin,
    MapSkin,
    WardSkin,
    Emote,
    SummonerIcon,
    Companion,
    Ui,
    Hud,
    Font,
    Sfx,
    Announcer,
    Structure,
    Minion,
    JungleMonster,
    Misc,
}

impl WellKnownModTag {
    /// Every well-known tag, so a caller can list or validate against them.
    pub const ALL: [WellKnownModTag; 17] = [
        WellKnownModTag::LeagueOfLegends,
        WellKnownModTag::Tft,
        WellKnownModTag::ChampionSkin,
        WellKnownModTag::MapSkin,
        WellKnownModTag::WardSkin,
        WellKnownModTag::Emote,
        WellKnownModTag::SummonerIcon,
        WellKnownModTag::Companion,
        WellKnownModTag::Ui,
        WellKnownModTag::Hud,
        WellKnownModTag::Font,
        WellKnownModTag::Sfx,
        WellKnownModTag::Announcer,
        WellKnownModTag::Structure,
        WellKnownModTag::Minion,
        WellKnownModTag::JungleMonster,
        WellKnownModTag::Misc,
    ];

    /// The tag's spelling in a config file.
    pub fn as_str(self) -> &'static str {
        match self {
            WellKnownModTag::LeagueOfLegends => "league-of-legends",
            WellKnownModTag::Tft => "tft",
            WellKnownModTag::ChampionSkin => "champion-skin",
            WellKnownModTag::MapSkin => "map-skin",
            WellKnownModTag::WardSkin => "ward-skin",
            WellKnownModTag::Emote => "emote",
            WellKnownModTag::SummonerIcon => "summoner-icon",
            WellKnownModTag::Companion => "companion",
            WellKnownModTag::Ui => "ui",
            WellKnownModTag::Hud => "hud",
            WellKnownModTag::Font => "font",
            WellKnownModTag::Sfx => "sfx",
            WellKnownModTag::Announcer => "announcer",
            WellKnownModTag::Structure => "structure",
            WellKnownModTag::Minion => "minion",
            WellKnownModTag::JungleMonster => "jungle-monster",
            WellKnownModTag::Misc => "misc",
        }
    }

    /// The tag a config file spelling names, if it names one.
    ///
    /// Returns `None` rather than an error: an unrecognized spelling is a
    /// [`ModTag::Custom`], not a failure.
    pub fn from_name(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tag| tag.as_str() == value)
    }
}

impl fmt::Display for WellKnownModTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A mod tag, either a well-known category or a custom string.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Hash)]
#[serde(untagged)]
pub enum ModTag {
    Known(WellKnownModTag),
    Custom(String),
}

impl fmt::Display for ModTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModTag::Known(tag) => f.write_str(tag.as_str()),
            ModTag::Custom(s) => f.write_str(s),
        }
    }
}

impl From<&str> for ModTag {
    fn from(s: &str) -> Self {
        match WellKnownModTag::from_name(s) {
            Some(tag) => ModTag::Known(tag),
            None => ModTag::Custom(s.to_owned()),
        }
    }
}

impl From<String> for ModTag {
    fn from(s: String) -> Self {
        match WellKnownModTag::from_name(&s) {
            Some(tag) => ModTag::Known(tag),
            None => ModTag::Custom(s),
        }
    }
}

/// Well-known game maps.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum WellKnownMap {
    SummonersRift,
    Aram,
    TeamfightTactics,
    Arena,
    Swarm,
}

impl WellKnownMap {
    /// Every well-known map, so a caller can list or validate against them.
    pub const ALL: [WellKnownMap; 5] = [
        WellKnownMap::SummonersRift,
        WellKnownMap::Aram,
        WellKnownMap::TeamfightTactics,
        WellKnownMap::Arena,
        WellKnownMap::Swarm,
    ];

    /// The map's spelling in a config file.
    pub fn as_str(self) -> &'static str {
        match self {
            WellKnownMap::SummonersRift => "summoners-rift",
            WellKnownMap::Aram => "aram",
            WellKnownMap::TeamfightTactics => "teamfight-tactics",
            WellKnownMap::Arena => "arena",
            WellKnownMap::Swarm => "swarm",
        }
    }

    /// The map a config file spelling names, if it names one.
    ///
    /// Returns `None` rather than an error, as [`WellKnownModTag::from_name`].
    pub fn from_name(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|map| map.as_str() == value)
    }
}

impl fmt::Display for WellKnownMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A map identifier, either a well-known map or a custom string.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Hash)]
#[serde(untagged)]
pub enum ModMap {
    Known(WellKnownMap),
    Custom(String),
}

impl fmt::Display for ModMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModMap::Known(map) => f.write_str(map.as_str()),
            ModMap::Custom(s) => f.write_str(s),
        }
    }
}

impl From<&str> for ModMap {
    fn from(s: &str) -> Self {
        match WellKnownMap::from_name(s) {
            Some(map) => ModMap::Known(map),
            None => ModMap::Custom(s.to_owned()),
        }
    }
}

impl From<String> for ModMap {
    fn from(s: String) -> Self {
        match WellKnownMap::from_name(&s) {
            Some(map) => ModMap::Known(map),
            None => ModMap::Custom(s),
        }
    }
}

/// Describes a mod project configuration file
///
/// `Default` is for struct update syntax
/// (`ModProject { name, ..Default::default() }`). A default project is empty,
/// not valid.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct ModProject {
    /// The name of the mod
    /// Must not contain spaces or special characters except for underscores and hyphens
    ///
    /// Example: `my_mod`
    pub name: String,

    /// The display name of the mod.
    ///
    /// Example: `My Mod`
    pub display_name: String,

    /// The version of the mod
    ///
    /// Example: `1.0.0`
    pub version: String,

    /// The description of the mod
    ///
    /// Example: `This is a mod for my game`
    pub description: String,

    /// The authors of the mod
    pub authors: Vec<ModProjectAuthor>,

    /// The license of the mod
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<ModProjectLicense>,

    /// Tags/categories for the mod (e.g., "champion-skin", "sfx")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<ModTag>,

    /// Champions this mod targets (e.g., "Aatrox", "Ahri")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub champions: Vec<String>,

    /// Maps this mod targets (e.g., "summoners-rift", "howling-abyss")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<ModMap>,

    /// File transformers to be applied during the build process
    /// Optional field - if not provided, no transformers will be applied
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transformers: Vec<FileTransformer>,

    /// Layers of the mod project
    /// Layers are loaded in order of priority (highest priority last)
    /// If not specified, a default "base" layer with priority 0 is assumed
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<ModProjectLayer>,

    /// The thumbnail file path relative to the mod project folder
    /// Optional field - if not specified, default thumbnail will be used
    ///
    /// Example: `thumbnail.webp`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,

    /// The embedded hashtables the project declares.
    ///
    /// The manifest is authoritative: a file under `hashes/` no entry here
    /// declares does not exist for lookup. By convention the files live at
    /// `hashes/{category}.hashes.txt` (see [`HASHES_DIR_NAME`]), but each
    /// entry's `path` is what says where its table is. Omitted when empty so
    /// a project without tables serializes as before this field existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hashtables: Vec<ModProjectHashtable>,
}

impl ModProject {
    /// Load a mod project from a project directory.
    ///
    /// Searches for `mod.config.json` (preferred) then `mod.config.toml`, and
    /// parses the first one found.
    pub fn load(project_dir: &Utf8Path) -> Result<Self, ModProjectError> {
        for format in ConfigFormat::ALL {
            let path = project_dir.join(format.file_name());
            if path.exists() {
                return Self::load_from_file_as(&path, format);
            }
        }

        Err(ModProjectError::ConfigNotFound(project_dir.to_owned()))
    }

    /// Load a mod project from a config file, taking the format from its
    /// extension.
    pub fn load_from_file(path: &Utf8Path) -> Result<Self, ModProjectError> {
        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            ModProjectError::UnsupportedExtension(path.extension().unwrap_or_default().to_owned())
        })?;

        Self::load_from_file_as(path, format)
    }

    /// Load a mod project from a config file parsed as `format`, whatever the
    /// file is named.
    ///
    /// Use this for a config whose name does not carry a usable extension.
    pub fn load_from_file_as(
        path: &Utf8Path,
        format: ConfigFormat,
    ) -> Result<Self, ModProjectError> {
        let content =
            std::fs::read_to_string(path).map_err(|source| ModProjectError::io(path, source))?;

        match format {
            ConfigFormat::Json => {
                serde_json::from_str(&content).map_err(|source| ModProjectError::Json {
                    path: path.to_owned(),
                    source,
                })
            }
            ConfigFormat::Toml => {
                toml::from_str(&content).map_err(|source| ModProjectError::Toml {
                    path: path.to_owned(),
                    source: Box::new(source),
                })
            }
        }
    }

    /// Write the project to a config file, taking the format from its
    /// extension.
    pub fn save(&self, path: &Utf8Path) -> Result<(), ModProjectError> {
        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            ModProjectError::UnsupportedExtension(path.extension().unwrap_or_default().to_owned())
        })?;

        self.save_as(path, format)
    }

    /// Write the project to `path` in `format`, whatever the file is named.
    pub fn save_as(&self, path: &Utf8Path, format: ConfigFormat) -> Result<(), ModProjectError> {
        let content = self.to_config_string(format)?;

        std::fs::write(path, content).map_err(|source| ModProjectError::io(path, source))
    }

    /// The file name a packed mod gets in `format`.
    ///
    /// `custom_name` is used as given, gaining the extension if it lacks it.
    /// Without one the name is `{name}_{version}.{extension}`.
    pub fn package_file_name(&self, custom_name: Option<String>, format: PackageFormat) -> String {
        let suffix = format!(".{}", format.extension());

        match custom_name {
            Some(name) if name.ends_with(&suffix) => name,
            Some(name) => name + &suffix,
            None => format!("{}_{}{}", self.name, self.version, suffix),
        }
    }

    /// The project's layers other than the base layer.
    ///
    /// Useful for warning about data loss when targeting a format that only
    /// stores the base layer, like Fantome.
    pub fn non_base_layers(&self) -> Vec<&ModProjectLayer> {
        self.layers
            .iter()
            .filter(|layer| !layer.is_base())
            .collect()
    }

    /// Render the project as the text of a config file.
    ///
    /// JSON is pretty-printed; the result is a file a mod author edits by hand.
    pub fn to_config_string(&self, format: ConfigFormat) -> Result<String, ModProjectError> {
        let result = match format {
            ConfigFormat::Json => serde_json::to_string_pretty(self).map_err(SerializeError::Json),
            ConfigFormat::Toml => toml::to_string_pretty(self).map_err(SerializeError::Toml),
        };

        result.map_err(|source| ModProjectError::Serialize { format, source })
    }
}

/// One `hashtables` manifest entry in a mod project config.
///
/// The project spelling of `ltk_hashtable`'s
/// [`HashtableEntry`](ltk_hashtable::HashtableEntry): lowercase
/// keys, `path` relative to the project root. Convert with
/// [`ModProjectHashtable::to_entry`] and [`ModProjectHashtable::from_entry`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ModProjectHashtable {
    /// Where the table file lives, relative to the project root.
    ///
    /// Example: `hashes/game.hashes.txt`
    pub path: String,
    /// The lookup domain of the table's names.
    pub category: ltk_hashtable::Category,
    /// The hash function keying the table's names.
    pub algorithm: ltk_hashtable::Algorithm,
    /// The declared key width in bits.
    pub bits: u8,
}

impl ModProjectHashtable {
    /// The entry as `ltk_hashtable`'s domain type.
    ///
    /// `None` when `bits` declares a width no key can have; the standard
    /// requires `1..=64`.
    pub fn to_entry(&self) -> Option<ltk_hashtable::HashtableEntry> {
        let width = ltk_hashtable::KeyWidth::new(self.bits)?;
        Some(ltk_hashtable::HashtableEntry::new(
            self.path.as_str(),
            self.category.clone(),
            self.algorithm.clone(),
            width,
        ))
    }

    /// Spell a domain entry the project way.
    pub fn from_entry(entry: &ltk_hashtable::HashtableEntry) -> Self {
        Self {
            path: entry.path().to_string(),
            category: entry.category().clone(),
            algorithm: entry.algorithm().clone(),
            bits: entry.width().bits(),
        }
    }
}

/// Represents a layer in a mod project
///
/// As with [`ModProject`], `Default` is for struct update syntax; it has no
/// name. Use [`base`](ModProjectLayer::base) for the layer every project has.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct ModProjectLayer {
    /// The name of the layer
    /// Must not contain spaces or special characters except for underscores and hyphens
    ///
    /// Example: `base`, `high_res_textures`, `gameplay_overhaul`
    pub name: String,

    /// Optional human-readable display name for the layer
    ///
    /// Example: `Base`, `High Res Textures`, `Gameplay Overhaul`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// The priority of the layer
    /// Higher priority layers override lower priority layers when they modify the same files
    /// Default is 0 for the base layer
    pub priority: i32,

    /// Optional description of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// String overrides for this layer, organized by locale.
    /// Outer key: locale (e.g., "en_us", "ko_kr", "zh_cn", or "default" for all locales)
    /// Inner map: field name (from lol.stringtable) -> new string value
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub string_overrides: IndexMap<String, IndexMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged)]
pub enum ModProjectAuthor {
    Name(String),
    Role { name: String, role: String },
}

/// How a project declares its license terms.
///
/// `url` being optional leaves `name` as the only required field, so without
/// `deny_unknown_fields` a misspelled key would make the object still match
/// `Custom`: `{"name": "X", "ur1": "..."}` would silently parse as a license
/// with no URL instead of failing. Rejecting unknown fields restores the
/// structural check that the required `url` used to provide.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(untagged, deny_unknown_fields)]
pub enum ModProjectLicense {
    Spdx(String),
    Custom {
        name: String,
        /// Optional link to the full terms. A project may name a license and
        /// ship its text in a `LICENSE` file without pointing anywhere.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
}

/// Represents a file transformer that can be applied to files during the build process
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct FileTransformer {
    /// The name of the transformer to use.
    pub name: String,

    /// File patterns to apply this transformer to.
    /// At least one of `patterns` or `files` must be provided
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,

    /// Specific files to apply this transformer to.
    /// At least one of `patterns` or `files` must be provided
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,

    /// Transformer-specific configuration
    /// This is an optional field that can be used to configure the transformer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<FileTransformerOptions>,
}

pub type FileTransformerOptions = HashMap<String, serde_json::Value>;

impl ModProjectLayer {
    /// The name every project's lowest-priority layer carries.
    pub const BASE_NAME: &'static str = "base";

    /// The directory a Fantome archive's `RAW/` entries import into, inside
    /// the base layer.
    ///
    /// Its files are named by game asset path rather than by their location
    /// inside a WAD, and are routed to a WAD when an overlay is built. The
    /// Fantome format carries no layers, so only the base layer has one.
    pub const RAW_DIR_NAME: &'static str = "raw";

    /// Composes the path to a layer's content directory, under `project_root`.
    pub fn content_path(project_root: &Utf8Path, layer_name: &str) -> Utf8PathBuf {
        project_root.join(CONTENT_DIR_NAME).join(layer_name)
    }

    /// Composes the path to a layer's raw content directory, under `project_root`.
    ///
    /// See [`RAW_DIR_NAME`](Self::RAW_DIR_NAME) for what lands there.
    pub fn raw_content_path(project_root: &Utf8Path) -> Utf8PathBuf {
        Self::content_path(project_root, Self::BASE_NAME).join(Self::RAW_DIR_NAME)
    }

    /// Returns the default base layer
    pub fn base() -> Self {
        Self {
            name: Self::BASE_NAME.to_string(),
            description: Some("Base layer of the mod".to_string()),
            ..Default::default()
        }
    }

    /// Whether this is the base layer, the one every project has.
    pub fn is_base(&self) -> bool {
        self.name == Self::BASE_NAME
    }

    /// The layer table a project has when it declares none: the base layer
    /// alone.
    pub fn default_table() -> Vec<Self> {
        vec![Self::base()]
    }

    /// Put a layer table read out of an archive into the order a project stores
    /// it.
    ///
    /// Adds the base layer when the archive names none, then sorts: base first,
    /// then by ascending priority, then by name as a person reads it - `layer9`
    /// before `layer10`, not after it. Both archive formats store
    /// their layers unordered - Fantome as a JSON object, modpkg as a hashed
    /// table - so without a sort here two imports of one mod would write two
    /// different config files. Every conversion into a [`ModProject`] goes
    /// through this, so no two of them can disagree about the order.
    ///
    /// An empty table becomes [`default_table`](Self::default_table).
    ///
    /// Two things an archive declares are corrected rather than carried
    /// through, because a project holding either is one [`ProjectPacker`]
    /// refuses and so could be imported but never packed again: a base layer
    /// whose priority is not 0, and a name declared twice, which is one
    /// directory claimed by two entries.
    ///
    /// This is deliberately something a caller asks for rather than something a
    /// table always holds. A config an author wrote by hand gets its mistakes
    /// reported by the packer - `PackError::InvalidBaseLayerPriority` names the
    /// priority it found - and silently rewriting one on load would take that
    /// message away. Only a table decoded from an archive, where no one is
    /// around to be told, is normalized.
    pub fn normalize_table(table: &mut Vec<Self>) {
        for layer in table.iter_mut().filter(|layer| layer.is_base()) {
            layer.priority = 0;
        }

        if !table.iter().any(Self::is_base) {
            table.push(Self::base());
        }

        table.sort_by(|a, b| {
            b.is_base()
                .cmp(&a.is_base())
                .then_with(|| a.priority.cmp(&b.priority))
                .then_with(|| natural_cmp(&a.name, &b.name))
        });

        // After the sort, so which of a repeated name survives is the order
        // above rather than however the archive's map iterated. Not `dedup_by`:
        // two entries sharing a name but not a priority do not end up adjacent.
        let mut seen = HashSet::new();
        table.retain(|layer| seen.insert(layer.name.clone()));
    }
}

/// Compare two names the way a person reads them, so `layer9` sorts before
/// `layer10`.
///
/// Runs of digits compare by the number they spell rather than character by
/// character, which is the only difference from [`str`]'s own ordering. It
/// matters wherever a layer table is shown: plain ordering puts `layer10`
/// second because `1` precedes `9`, which reads as a mistake to everyone who
/// sees it.
///
/// Leading zeros carry no value, so `09` and `9` spell the same number; names
/// that tie on every run fall back to plain ordering, which keeps this a total
/// order rather than calling two different names equal.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_cmp_bytes(a.as_bytes(), b.as_bytes()).then_with(|| a.cmp(b))
}

/// The digit-aware half of [`natural_cmp`], which returns [`Ordering::Equal`]
/// for names differing only in how their numbers are padded.
fn natural_cmp_bytes(mut a: &[u8], mut b: &[u8]) -> Ordering {
    loop {
        return match (a.first(), b.first()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x_digits, a_rest) = split_number(a);
                let (y_digits, b_rest) = split_number(b);

                // Longer means larger once the padding is gone, so the numbers
                // never have to be parsed and a run of any length is safe.
                match x_digits
                    .len()
                    .cmp(&y_digits.len())
                    .then_with(|| x_digits.cmp(y_digits))
                {
                    Ordering::Equal => {
                        (a, b) = (a_rest, b_rest);
                        continue;
                    }
                    ordering => ordering,
                }
            }
            (Some(x), Some(y)) if x == y => {
                (a, b) = (&a[1..], &b[1..]);
                continue;
            }
            (Some(x), Some(y)) => x.cmp(y),
        };
    }
}

/// Split a leading run of digits off `name`, dropping the zeros that pad it.
fn split_number(name: &[u8]) -> (&[u8], &[u8]) {
    let end = name
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(name.len());
    let digits = &name[..end];
    let value_start = digits
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(digits.len());

    (&digits[value_start..], &name[end..])
}

#[cfg(test)]
mod tests;
