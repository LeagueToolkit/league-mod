//! The verifier side, end to end: build platform trust from a public key
//! and a signed root, attest every statement through the membership lane,
//! then run the base-skin diagnostic on every champion WAD in the overlay.

use std::fs::File;
use std::io::BufReader;

use camino::{Utf8Path, Utf8PathBuf};
use miette::{IntoDiagnostic, WrapErr, miette};

use ltk_sig::base_skin::{self, ResolveError, SkinRefStatus};
use ltk_sig::io::bundle::Bundle;
use ltk_sig::trust::{AttestContext, AttestationPolicy, Platform, PlatformTrust};
use ltk_sig::verify::{FileCheck, VerifyContext};
use ltk_wad::Wad;

use crate::util::{decode_hex32, load_signing_key, unix_now};

pub struct VerifyArgs {
    pub game_dir: Utf8PathBuf,
    pub overlay: Utf8PathBuf,
    pub bundle: Utf8PathBuf,
    pub pub_key: Option<String>,
    pub key: Option<Utf8PathBuf>,
}

pub fn run(args: &VerifyArgs) -> miette::Result<()> {
    // ---- baked side: the platform key is the only trusted input ----
    let pub_key = match (&args.pub_key, &args.key) {
        (Some(hex), _) => decode_hex32(hex)?,
        (None, Some(key_path)) => load_signing_key(key_path)?.verifying_key().to_bytes(),
        (None, None) => return Err(miette!("either --pub-key or --key is required")),
    };
    let platform = Platform {
        name: "corpus".to_owned(),
        pub_key,
        url: "https://example.invalid/ltk".to_owned(),
        min_issued_at: 0,
        accepts_roots: true,
        attestations: AttestationPolicy::None,
    };
    let trust = PlatformTrust::new(platform, None).into_diagnostic()?;

    // ---- untrusted side: everything else arrives in one bundle ----
    let bundle_bytes = std::fs::read(args.bundle.as_std_path())
        .into_diagnostic()
        .wrap_err_with(|| format!("reading bundle {}", args.bundle))?;
    let bundle = Bundle::from_bytes(&bundle_bytes)
        .into_diagnostic()
        .wrap_err_with(|| format!("parsing bundle {}", args.bundle))?;

    let assembly = VerifyContext::from_bundle(
        vec![trust],
        &bundle,
        &AttestContext {
            now: unix_now(),
            account_id: None,
        },
    );
    for defect in &assembly.defects {
        println!(
            "BUNDLE DEFECT [{:?} #{}]: {}",
            defect.kind, defect.index, defect.error
        );
    }
    for (index, rejected) in &assembly.rejected_statements {
        println!("NOT ATTESTED: bundle statement #{index}");
        for rejection in &rejected.rejections {
            println!(
                "  [{} / evidence {}] {}",
                rejection.platform, rejection.evidence, rejection.error
            );
        }
    }
    let context = assembly.context;
    println!(
        "statements attested: {}/{}",
        context.endorsements().len(),
        bundle.statements().len()
    );

    // ---- walk the overlay and verify every champion WAD ----
    let attested = |name_hash: u64, checksum: u64| {
        context
            .attested(name_hash, FileCheck::ChecksumCompressed(checksum))
            .is_some()
    };

    let mut checked = 0usize;
    let mut clean = 0usize;
    let mut vanilla = 0usize;
    let mut violations = 0usize;
    let mut skipped = 0usize;

    for entry in walkdir::WalkDir::new(args.overlay.as_std_path()) {
        let entry = entry.into_diagnostic()?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = Utf8PathBuf::from_path_buf(entry.path().to_path_buf())
            .map_err(|p| miette!("non-UTF-8 path: {}", p.display()))?;
        let Some(file_name) = path.file_name() else {
            continue;
        };
        if !file_name.to_ascii_lowercase().ends_with(".wad.client") {
            continue;
        }
        let rel = path
            .strip_prefix(&args.overlay)
            .map_err(|_| miette!("{path} is not under the overlay root"))?;
        let original_path = args.game_dir.join(rel);
        if !original_path.as_std_path().exists() {
            println!("SKIP {rel}: no matching original game WAD");
            skipped += 1;
            continue;
        }
        let Some(champion) = base_skin::champion_from_wad_filename(file_name) else {
            skipped += 1;
            continue;
        };

        let original = mount_wad(&original_path)?;
        let mut merged = mount_wad(&path)?;

        match base_skin::verify_base_skin(&original, &mut merged, &champion, &attested) {
            Ok(None) => {
                println!("VANILLA   {rel}: base-skin bin unmodified, check skipped");
                vanilla += 1;
            }
            Ok(Some(report)) => {
                checked += 1;
                let verdict = if report.is_clean() {
                    "CLEAN"
                } else {
                    "VIOLATION"
                };
                println!(
                    "{verdict:9} {rel}: entry via '{}' (bin {:?})",
                    report.bin_path, report.bin_status
                );
                for reference in &report.refs {
                    let marker = match reference.status {
                        SkinRefStatus::Attested => "attested  ",
                        SkinRefStatus::Unattested => "UNATTESTED",
                        SkinRefStatus::Missing => "MISSING   ",
                        SkinRefStatus::Unmodified => "unmodified",
                    };
                    println!(
                        "    {marker} {} ({:016x})",
                        reference.path, reference.name_hash
                    );
                }
                if report.is_clean() {
                    clean += 1;
                } else {
                    violations += 1;
                }
            }
            Err(ResolveError::RootBinMissing { .. }) => {
                // Not a champion WAD (or no base skin data): nothing to verify.
                skipped += 1;
            }
            Err(other) => {
                println!("ERROR     {rel}: {other}");
                violations += 1;
                checked += 1;
            }
        }
    }

    println!(
        "\nchampion WADs checked: {checked} ({clean} clean, {violations} violation(s)), \
         {vanilla} vanilla, {skipped} skipped"
    );
    Ok(())
}

fn mount_wad(path: &Utf8Path) -> miette::Result<Wad<BufReader<File>>> {
    let file = File::open(path.as_std_path())
        .into_diagnostic()
        .wrap_err_with(|| format!("opening WAD {path}"))?;
    Wad::mount(BufReader::new(file))
        .into_diagnostic()
        .wrap_err_with(|| format!("mounting WAD {path}"))
}
