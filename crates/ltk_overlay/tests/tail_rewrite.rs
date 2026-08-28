//! Rebuilding an overlay WAD by rewriting only its tail.
//!
//! Two things are worth proving here, and they pull in opposite directions.
//! The first is that the fast path is actually taken and produces a WAD
//! indistinguishable from a full rebuild. The second is that every trust check
//! guarding it really does fall back to the full rebuild, because the cost of a
//! wrong answer is a corrupt archive the game will reject.
//!
//! Which path ran is observed without a test hook, from the file itself: the
//! copied source region still holds the *original* bytes of every chunk an
//! override replaced, unreferenced by any TOC entry. Stamping over those bytes
//! after a build gives a marker that survives a tail rewrite (which never
//! touches the region) and vanishes on a full rebuild (which copies the region
//! afresh).

mod common;

use camino::{Utf8Path, Utf8PathBuf};
use common::{assert_chunks_equivalent, assert_wad_is_well_formed, write_game_wad, write_mod_dir};
use ltk_overlay::utils::resolve_chunk_hash;
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuilder, OverlayState};
use ltk_wad::{Wad, WadHash};
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::ops::Range;

const WAD_NAME: &str = "Aatrox.wad.client";
const WAD_REL: &str = "DATA/FINAL/Champions/Aatrox.wad.client";
const SKIN: &str = "assets/characters/aatrox/skin0.tex";
const VFX: &str = "assets/characters/aatrox/vfx.tex";
const SOUND: &str = "assets/characters/aatrox/sfx.bin";

const SKIN_ORIGINAL: &[u8] = b"the game's own skin texture, long enough to be worth compressing";
const VFX_ORIGINAL: &[u8] = b"the game's own vfx texture, also reasonably long for the region";
const SOUND_ORIGINAL: &[u8] = b"the game's own sound bank, third chunk of the fixture WAD here";

/// The marker byte stamped over unreferenced region bytes.
const STAMP: u8 = 0xAB;

/// An override's first and second versions.
///
/// The two differ in *length* on purpose: a mod's content fingerprint is built
/// from file sizes and mtimes, and two writes inside one mtime tick would look
/// identical if the size stayed the same - the build would then take the
/// exact-match skip and never reach the code under test.
const EDIT_V1: &[u8] = b"SKIN_V1";
const EDIT_V2: &[u8] = b"SKIN_V2_WHICH_IS_LONGER";

fn hash(path: &str) -> u64 {
    resolve_chunk_hash(Utf8Path::new(path), b"").expect("chunk path hashes")
}

/// A game directory with one champion WAD holding three chunks.
fn write_game(root: &Utf8Path) -> Utf8PathBuf {
    let game_dir = root.join("Game");
    write_game_wad(
        &game_dir.join(WAD_REL),
        &[
            (SKIN, SKIN_ORIGINAL),
            (VFX, VFX_ORIGINAL),
            (SOUND, SOUND_ORIGINAL),
        ],
    );
    game_dir
}

/// A profile: the game, a mod, and the overlay built from them.
struct Profile {
    game_dir: Utf8PathBuf,
    profile_dir: Utf8PathBuf,
    overlay_root: Utf8PathBuf,
    mod_dir: Utf8PathBuf,
}

impl Profile {
    fn new(root: &Utf8Path, overrides: &[(&str, &[u8])]) -> Self {
        let game_dir = write_game(root);
        let profile_dir = root.join("profile");
        Self {
            overlay_root: profile_dir.join("overlay"),
            mod_dir: write_mod_dir(root, "a-mod", WAD_NAME, overrides),
            game_dir,
            profile_dir,
        }
    }

    fn build(&self) -> ltk_overlay::OverlayBuildResult {
        let mut builder = OverlayBuilder::new(
            self.game_dir.clone(),
            self.overlay_root.clone(),
            self.profile_dir.clone(),
        );
        builder.set_enabled_mods(vec![EnabledMod {
            id: "a-mod".to_string(),
            content: Box::new(FsModContent::new(self.mod_dir.clone())),
            enabled_layers: None,
        }]);
        builder.build().expect("overlay builds")
    }

    /// Build with a caller-supplied content provider, surfacing failures.
    fn try_build_with(
        &self,
        content: Box<dyn ltk_overlay::ModContentProvider>,
    ) -> ltk_overlay::Result<ltk_overlay::OverlayBuildResult> {
        let mut builder = OverlayBuilder::new(
            self.game_dir.clone(),
            self.overlay_root.clone(),
            self.profile_dir.clone(),
        );
        builder.set_enabled_mods(vec![EnabledMod {
            id: "a-mod".to_string(),
            content,
            enabled_layers: None,
        }]);
        builder.build()
    }

    fn overlay_wad(&self) -> Utf8PathBuf {
        self.overlay_root.join(WAD_REL)
    }

    fn state_path(&self) -> Utf8PathBuf {
        self.profile_dir.join("overlay.json")
    }

    fn state(&self) -> OverlayState {
        OverlayState::load(&self.state_path())
            .expect("state parses")
            .expect("a build wrote state")
    }

    /// The key the state stores this WAD under.
    ///
    /// Game-relative paths are assembled with the platform's separator, so the
    /// key is `DATA\FINAL\...` on Windows and `DATA/FINAL/...` elsewhere.
    fn layout_key(&self) -> String {
        self.state()
            .wad_layouts
            .keys()
            .find(|key| key.ends_with(WAD_NAME))
            .expect("the build recorded a layout for the champion WAD")
            .clone()
    }

    fn save_state(&self, state: &OverlayState) {
        state.save(&self.state_path()).expect("state is writable");
    }

    fn layout_record(&self) -> ltk_overlay::state::WadLayoutRecord {
        self.state()
            .wad_layout(&self.layout_key())
            .expect("the build recorded a usable layout")
            .clone()
    }

    fn write_override(&self, chunk_path: &str, bytes: &[u8]) {
        let file = self
            .mod_dir
            .join("content")
            .join("base")
            .join(WAD_NAME)
            .join(chunk_path);
        fs::create_dir_all(file.parent().unwrap().as_std_path()).unwrap();
        fs::write(file.as_std_path(), bytes).expect("override is writable");
    }

    /// Where `chunk_path`'s original bytes still sit inside the copied region,
    /// unreferenced because an override replaced the chunk.
    fn shadow_range(&self, chunk_path: &str) -> Range<usize> {
        let record = self.layout_record();
        let source = Wad::mount(fs::File::open(self.game_dir.join(WAD_REL).as_std_path()).unwrap())
            .expect("game WAD mounts");
        let chunk = *source
            .chunks()
            .get(WadHash(hash(chunk_path)))
            .expect("the game WAD holds the chunk");

        let start = (chunk.data_offset as i64 + record.layout.offset_delta) as usize;
        start..start + chunk.compressed_size
    }

    /// Stamp over an overridden chunk's shadow bytes, so a later build reveals
    /// which path it took.
    fn stamp_shadow(&self, chunk_path: &str) {
        let range = self.shadow_range(chunk_path);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(self.overlay_wad().as_std_path())
            .expect("overlay WAD is writable");
        file.seek(SeekFrom::Start(range.start as u64)).unwrap();
        file.write_all(&vec![STAMP; range.len()]).unwrap();
    }

    /// Whether the stamp survived, i.e. the copied region was never rewritten.
    fn shadow_is_stamped(&self, chunk_path: &str) -> bool {
        let range = self.shadow_range(chunk_path);
        let bytes = fs::read(self.overlay_wad().as_std_path()).expect("overlay WAD is readable");
        bytes[range].iter().all(|&byte| byte == STAMP)
    }

    /// Read one chunk's decompressed bytes out of the overlay WAD.
    fn overlay_chunk(&self, chunk_path: &str) -> Option<Vec<u8>> {
        let mut wad =
            Wad::mount(fs::File::open(self.overlay_wad().as_std_path()).unwrap()).unwrap();
        let chunk = *wad.chunks().get(WadHash(hash(chunk_path)))?;
        Some(wad.load_chunk_decompressed(&chunk).unwrap().to_vec())
    }
}

/// A mod whose pass-2 reads fail, so a build dies after the dirty markers are
/// written but before any WAD is finished.
///
/// Everything else delegates to a real [`FsModContent`], so pass 1, the
/// fingerprints, and the routing all behave normally and the build gets far
/// enough to plan its tail rewrites.
struct FailsInPassTwo(FsModContent);

impl ltk_overlay::ModContentProvider for FailsInPassTwo {
    fn mod_project(&mut self) -> ltk_overlay::Result<ltk_mod_project::ModProject> {
        self.0.mod_project()
    }

    fn list_layer_wads(&mut self, layer: &str) -> ltk_overlay::Result<Vec<String>> {
        self.0.list_layer_wads(layer)
    }

    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> ltk_overlay::Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        self.0.read_wad_overrides(layer, wad_name)
    }

    fn read_raw_overrides(&mut self) -> ltk_overlay::Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        self.0.read_raw_overrides()
    }

    fn content_fingerprint(&self) -> ltk_overlay::Result<Option<u64>> {
        self.0.content_fingerprint()
    }

    fn read_wad_override_file(
        &mut self,
        _layer: &str,
        _wad_name: &str,
        _rel_path: &Utf8Path,
    ) -> ltk_overlay::Result<Vec<u8>> {
        Err(ltk_overlay::Error::Other("the mod vanished".to_string()))
    }

    fn read_raw_override_file(&mut self, _rel_path: &Utf8Path) -> ltk_overlay::Result<Vec<u8>> {
        Err(ltk_overlay::Error::Other("the mod vanished".to_string()))
    }
}

/// Build the same final mod state from scratch in a separate profile, for
/// comparison against an incrementally-updated overlay.
fn build_from_scratch(root: &Utf8Path, overrides: &[(&str, &[u8])]) -> Utf8PathBuf {
    let fresh_root = root.join("fresh");
    fs::create_dir_all(fresh_root.as_std_path()).unwrap();
    let fresh = Profile::new(&fresh_root, overrides);
    fresh.build();
    fresh.overlay_wad()
}

/// The headline: editing an override's bytes rewrites the tail instead of
/// copying the WAD, and the result is the same archive a full rebuild produces.
#[test]
fn editing_an_override_rewrites_only_the_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let profile = Profile::new(&root, &[(SKIN, b"MOD_V1")]);
    profile.build();
    profile.stamp_shadow(SKIN);

    profile.write_override(SKIN, b"MOD_V2_A_DIFFERENT_LENGTH");
    let rebuild = profile.build();

    assert_eq!(rebuild.wads_built.len(), 1);
    assert!(
        profile.shadow_is_stamped(SKIN),
        "the copied source region must survive an override's byte change"
    );
    assert_eq!(
        profile.overlay_chunk(SKIN).as_deref(),
        Some(b"MOD_V2_A_DIFFERENT_LENGTH".as_slice())
    );
    assert_wad_is_well_formed(&profile.overlay_wad());
    assert_chunks_equivalent(
        &profile.overlay_wad(),
        &build_from_scratch(&root, &[(SKIN, b"MOD_V2_A_DIFFERENT_LENGTH")]),
    );
}

/// A chain of edits stays correct: each rewrite starts from the file the
/// previous one left, and the tail never accumulates stale entries.
#[test]
fn repeated_edits_stay_equivalent_to_a_fresh_build() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let profile = Profile::new(&root, &[(SKIN, b"V1"), (VFX, b"VFX_V1")]);
    profile.build();
    profile.stamp_shadow(SKIN);

    for round in 2..=4u8 {
        profile.write_override(
            SKIN,
            format!("SKIN_V{round}").repeat(round as usize).as_bytes(),
        );
        profile.write_override(VFX, format!("VFX_V{round}").as_bytes());
        profile.build();
    }

    assert!(
        profile.shadow_is_stamped(SKIN),
        "every edit in the chain must keep the copied region"
    );
    assert_wad_is_well_formed(&profile.overlay_wad());
    assert_chunks_equivalent(
        &profile.overlay_wad(),
        &build_from_scratch(
            &root,
            &[(SKIN, b"SKIN_V4SKIN_V4SKIN_V4SKIN_V4"), (VFX, b"VFX_V4")],
        ),
    );
}

/// An override whose bytes did not change is neither re-read nor recompressed,
/// but must still come out of the rewritten tail intact.
#[test]
fn unchanged_overrides_survive_a_rewrite_of_their_neighbours() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    let profile = Profile::new(&root, &[(SKIN, EDIT_V1), (VFX, b"VFX_UNCHANGED_FOREVER")]);
    profile.build();
    profile.stamp_shadow(SKIN);

    profile.write_override(SKIN, EDIT_V2);
    profile.build();

    assert!(profile.shadow_is_stamped(SKIN));
    assert_eq!(
        profile.overlay_chunk(VFX).as_deref(),
        Some(b"VFX_UNCHANGED_FOREVER".as_slice()),
        "an untouched override must be carried over from the old tail unharmed"
    );
    assert_eq!(profile.overlay_chunk(SKIN).as_deref(), Some(EDIT_V2));
}

/// League validates a chunk shared across WADs by its compressed checksum, so
/// the copy a tail rewrite carries over and the copy a full rebuild compresses
/// from scratch must come out identical - in the same build, on different
/// paths, possibly under different zstd versions than the bytes were made with.
///
/// The reused bytes seed the compression memo for exactly this reason.
#[test]
fn a_reused_chunk_and_a_freshly_built_one_stay_byte_identical() {
    const AHRI_REL: &str = "DATA/FINAL/Champions/Ahri.wad.client";
    const SHARED: &str = "assets/shared/effect.tex";

    let tmp = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let game_dir = root.join("Game");
    let profile_dir = root.join("profile");
    let overlay_root = profile_dir.join("overlay");

    // The same chunk hash lives in two game WADs, so an override of it is
    // routed to both.
    write_game_wad(
        &game_dir.join(WAD_REL),
        &[(SHARED, b"SHARED_ORIGINAL"), (SKIN, SKIN_ORIGINAL)],
    );
    write_game_wad(&game_dir.join(AHRI_REL), &[(SHARED, b"SHARED_ORIGINAL")]);

    let mod_dir = write_mod_dir(
        &root,
        "shared-mod",
        WAD_NAME,
        &[
            (SHARED, b"SHARED_OVERRIDE_BYTES_LONG_ENOUGH_TO_COMPRESS"),
            (SKIN, EDIT_V1),
        ],
    );

    let build = || {
        let mut builder =
            OverlayBuilder::new(game_dir.clone(), overlay_root.clone(), profile_dir.clone());
        builder.set_enabled_mods(vec![EnabledMod {
            id: "shared-mod".to_string(),
            content: Box::new(FsModContent::new(mod_dir.clone())),
            enabled_layers: None,
        }]);
        builder.build().expect("overlay builds");
    };

    build();

    // Change only the chunk Aatrox alone holds, so Aatrox rebuilds while the
    // shared chunk is unchanged; then remove Ahri's overlay so it has to be
    // rebuilt in full. Aatrox reuses the shared chunk from its tail, Ahri
    // compresses it afresh, in the same build.
    fs::write(
        mod_dir
            .join("content")
            .join("base")
            .join(WAD_NAME)
            .join(SKIN)
            .as_std_path(),
        EDIT_V2,
    )
    .unwrap();
    fs::remove_file(overlay_root.join(AHRI_REL).as_std_path()).unwrap();

    build();

    let facts_of = |wad: &Utf8Path| {
        let mut wad = Wad::mount(fs::File::open(wad.as_std_path()).unwrap()).unwrap();
        let chunk = *wad
            .chunks()
            .get(WadHash(hash(SHARED)))
            .expect("both overlay WADs hold the shared chunk");
        (
            wad.load_chunk_raw(&chunk).unwrap().to_vec(),
            chunk.checksum,
            chunk.compression_type,
        )
    };

    assert_eq!(
        facts_of(&overlay_root.join(WAD_REL)),
        facts_of(&overlay_root.join(AHRI_REL)),
        "a chunk carried over from an old tail and the same chunk compressed \
         fresh must be byte-identical, or the game rejects the pair"
    );
}

/// Every precondition, violated on its own, must send the WAD down the full
/// rebuild path - and the WAD that comes out must still be right.
///
/// Each case stamps the region first, so a surviving stamp would mean the fast
/// path was taken when it should not have been.
mod fallbacks {
    use super::*;

    /// Set up a built profile with a stamped region and a pending edit, then
    /// let `sabotage` invalidate one precondition.
    fn after_sabotage(root: &Utf8Path, sabotage: impl FnOnce(&Profile)) -> Profile {
        let profile = Profile::new(root, &[(SKIN, EDIT_V1)]);
        profile.build();
        profile.stamp_shadow(SKIN);
        profile.write_override(SKIN, EDIT_V2);

        sabotage(&profile);

        profile.build();
        profile
    }

    /// The stamp must be gone (region recopied) and the content correct.
    fn assert_full_rebuild(profile: &Profile) {
        assert!(
            !profile.shadow_is_stamped(SKIN),
            "the WAD must have been rebuilt in full, recopying the source region"
        );
        assert_eq!(profile.overlay_chunk(SKIN).as_deref(), Some(EDIT_V2));
        assert_wad_is_well_formed(&profile.overlay_wad());
    }

    #[test]
    fn a_changed_game_wad_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            // Same chunk set, different bytes: only the TOC hash catches this.
            write_game_wad(
                &profile.game_dir.join(WAD_REL),
                &[
                    (SKIN, b"the game patched this texture to something else"),
                    (VFX, VFX_ORIGINAL),
                    (SOUND, SOUND_ORIGINAL),
                ],
            );
        });

        assert_full_rebuild(&profile);
    }

    #[test]
    fn a_truncated_overlay_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            let len = fs::metadata(profile.overlay_wad().as_std_path())
                .unwrap()
                .len();
            fs::OpenOptions::new()
                .write(true)
                .open(profile.overlay_wad().as_std_path())
                .unwrap()
                .set_len(len / 2)
                .unwrap();
        });

        assert_full_rebuild(&profile);
    }

    #[test]
    fn a_mutated_passthrough_toc_entry_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            let record = profile.layout_record();
            // Corrupt the compressed size of whichever entry sorts first.
            let entry = record.layout.toc_offset() + 12;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .open(profile.overlay_wad().as_std_path())
                .unwrap();
            file.seek(SeekFrom::Start(entry)).unwrap();
            file.write_all(&7u32.to_le_bytes()).unwrap();
        });

        assert_full_rebuild(&profile);
    }

    #[test]
    fn a_state_version_bump_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            let mut state = profile.state();
            state.version += 1;
            profile.save_state(&state);
        });

        assert_full_rebuild(&profile);
    }

    #[test]
    fn a_dirty_flag_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            let mut state = profile.state();
            state.dirty_wads.insert(profile.layout_key());
            profile.save_state(&state);
        });

        assert_full_rebuild(&profile);
    }

    #[test]
    fn a_missing_layout_record_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            let mut state = profile.state();
            state.wad_layouts.clear();
            profile.save_state(&state);
        });

        assert_full_rebuild(&profile);
    }

    #[test]
    fn an_unreadable_state_file_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            fs::write(profile.state_path().as_std_path(), b"{ torn json").unwrap();
        });

        assert_full_rebuild(&profile);
    }

    /// A build killed between the tail write and the TOC write leaves a torn
    /// WAD, marked dirty. The next build must rebuild it rather than trust it.
    #[test]
    fn a_torn_rewrite_is_repaired_by_the_next_build() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = after_sabotage(&root, |profile| {
            let record = profile.layout_record();
            // Cut the file where a rewrite would have been: tail written to,
            // TOC still describing the old one.
            fs::OpenOptions::new()
                .write(true)
                .open(profile.overlay_wad().as_std_path())
                .unwrap()
                .set_len(record.layout.tail_offset)
                .unwrap();

            let mut state = profile.state();
            state.dirty_wads.insert(profile.layout_key());
            profile.save_state(&state);
        });

        assert_full_rebuild(&profile);
    }

    /// The dirty markers are written *before* the patch pass, so a build that
    /// dies part-way leaves them on disk for the next one to find.
    ///
    /// This drives the marking through a real build rather than planting the
    /// flag by hand: the WAD is planned for a tail rewrite, then pass 2 fails.
    #[test]
    fn a_build_that_dies_after_planning_leaves_its_wads_marked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = Profile::new(&root, &[(SKIN, EDIT_V1)]);
        profile.build();
        profile.stamp_shadow(SKIN);
        let key = profile.layout_key();

        profile.write_override(SKIN, EDIT_V2);
        let failed = profile.try_build_with(Box::new(FailsInPassTwo(FsModContent::new(
            profile.mod_dir.clone(),
        ))));

        assert!(failed.is_err(), "pass 2 was supposed to fail");
        assert!(
            profile.state().dirty_wads.contains(&key),
            "a build interrupted after planning must leave its in-place WADs marked, \
             so the next build does not trust them"
        );

        // The next build finds the marker and rebuilds rather than reusing.
        profile.build();

        assert!(
            !profile.shadow_is_stamped(SKIN),
            "the marked WAD must be rebuilt in full"
        );
        assert_eq!(profile.overlay_chunk(SKIN).as_deref(), Some(EDIT_V2));
        assert!(profile.state().dirty_wads.is_empty());
        assert_wad_is_well_formed(&profile.overlay_wad());
    }

    /// A WAD left dirty by a killed build may be torn, so it must be rebuilt
    /// even when nothing about the mod changed - the case where the user
    /// reverts the edit that triggered the interrupted build, which would
    /// otherwise reach the exact-match skip or the per-WAD reuse path and serve
    /// the torn file to the game.
    #[test]
    fn a_dirty_wad_is_rebuilt_even_when_nothing_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = Profile::new(&root, &[(SKIN, EDIT_V1)]);
        profile.build();
        profile.stamp_shadow(SKIN);

        let mut state = profile.state();
        state.dirty_wads.insert(profile.layout_key());
        profile.save_state(&state);

        let rebuild = profile.build();

        assert_eq!(
            rebuild.wads_built.len(),
            1,
            "a dirty WAD must be rebuilt, not skipped or reused"
        );
        assert!(
            !profile.shadow_is_stamped(SKIN),
            "the rebuild must be a full one, recopying the source region"
        );
        assert_eq!(profile.overlay_chunk(SKIN).as_deref(), Some(EDIT_V1));
        assert!(profile.state().dirty_wads.is_empty());
        assert_wad_is_well_formed(&profile.overlay_wad());
    }

    /// Adding a chunk changes the WAD's entry count, which no longer fits the
    /// TOC the file reserved - the accepted limit while TOC slack is zero.
    #[test]
    fn adding_an_entry_forces_a_full_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = Profile::new(&root, &[(SKIN, EDIT_V1)]);
        profile.build();
        profile.stamp_shadow(SKIN);

        profile.write_override(SKIN, EDIT_V2);
        profile.write_override("assets/characters/aatrox/brand_new.bin", b"NEW ENTRY");
        profile.build();

        assert!(
            !profile.shadow_is_stamped(SKIN),
            "a change to the WAD's entry set must take the full rebuild path"
        );
        assert_eq!(
            profile
                .overlay_chunk("assets/characters/aatrox/brand_new.bin")
                .as_deref(),
            Some(b"NEW ENTRY".as_slice())
        );
        assert_wad_is_well_formed(&profile.overlay_wad());
    }

    /// Dropping an override also changes the entry count only when the chunk
    /// was a new entry; dropping an override of an existing chunk keeps the
    /// count, so it stays on the fast path and reverts to the game's bytes.
    #[test]
    fn dropping_an_override_reverts_to_the_game_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let profile = Profile::new(&root, &[(SKIN, EDIT_V1), (VFX, b"VFX_V1")]);
        profile.build();
        profile.stamp_shadow(VFX);

        // Swap which chunk is overridden: one added, one removed, count equal.
        fs::remove_file(
            profile
                .mod_dir
                .join("content")
                .join("base")
                .join(WAD_NAME)
                .join(SKIN)
                .as_std_path(),
        )
        .unwrap();
        profile.write_override(SOUND, b"SOUND_V1");
        profile.build();

        assert_eq!(
            profile.overlay_chunk(SKIN).as_deref(),
            Some(SKIN_ORIGINAL),
            "a dropped override must revert to the game's own bytes, which the \
             copied region still holds"
        );
        assert_eq!(
            profile.overlay_chunk(SOUND).as_deref(),
            Some(b"SOUND_V1".as_slice())
        );
        assert_wad_is_well_formed(&profile.overlay_wad());
        assert_chunks_equivalent(
            &profile.overlay_wad(),
            &build_from_scratch(&root, &[(VFX, b"VFX_V1"), (SOUND, b"SOUND_V1")]),
        );
    }
}
