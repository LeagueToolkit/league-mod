//! Ceremony and verification CLI for the `ltk_sig` platform-attested trust
//! model.
//!
//! Covers the platform-side ceremony (extract statements from mods, build
//! set files, sign roots) and the verifier side (build an overlay from mods,
//! then run the base-skin attestation check against it) so the whole trust
//! pipeline can be exercised end to end without a manager build.

use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use miette::Result;

mod commands;
mod util;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate an Ed25519 platform key (32-byte seed file).
    Keygen {
        /// Path to write the 32-byte private key seed to.
        #[arg(short, long)]
        output: Utf8PathBuf,
    },
    /// Print the hex public key derived from a private key seed.
    Pubkey {
        /// Path to the 32-byte private key seed.
        #[arg(short, long)]
        key: Utf8PathBuf,
    },
    /// Extract statements from mods: for each champion WAD whose skin0.bin
    /// a mod modifies, sign exactly the mesh references the base-skin check
    /// enforces, with checksums exactly as they will appear in a built
    /// overlay. Mods are untrusted input: archives are processed purely in
    /// memory, and per-mod failures (corrupt archives, non-champion mods,
    /// bogus WAD names) are recorded and skipped, never fatal to the batch.
    Extract {
        /// League `Game` directory (contains `DATA/FINAL`).
        #[arg(long)]
        game_dir: Utf8PathBuf,
        /// Directory to write statement tokens (`<hash>.token`) and
        /// `index.json` to.
        #[arg(short, long)]
        output: Utf8PathBuf,
        /// Directory to scan (recursively) for `.fantome` archives.
        #[arg(long)]
        mods_dir: Option<Utf8PathBuf>,
        /// Individual mod archives (`.fantome`).
        #[arg(required_unless_present = "mods_dir")]
        mods: Vec<Utf8PathBuf>,
    },
    /// Build a membership set file from a directory of statement tokens.
    BuildSet {
        /// Directory containing `.token` statement files.
        #[arg(long)]
        statements: Utf8PathBuf,
        /// Directory to write the content-addressed set file into
        /// (named by its hex SHA-256 digest).
        #[arg(short, long)]
        output: Utf8PathBuf,
    },
    /// Sign a root over one or more named set files.
    SignRoot {
        /// Path to the platform's 32-byte private key seed.
        #[arg(short, long)]
        key: Utf8PathBuf,
        /// Named set file, as `name=path`. Repeatable.
        #[arg(long = "set", required = true)]
        sets: Vec<String>,
        /// The current root token this one supersedes. The new issue time
        /// is derived as `max(now, predecessor + 1)`, so re-signing within
        /// one second (or with a stepped-back clock) can never mint an
        /// equivocating or stale root. Omit only for a platform's first
        /// root.
        #[arg(long)]
        predecessor: Option<Utf8PathBuf>,
        /// Path to write the signed root token to.
        #[arg(short, long)]
        output: Utf8PathBuf,
    },
    /// Build a WAD overlay from mods (thin wrapper over `ltk_overlay`).
    BuildOverlay {
        /// League `Game` directory (contains `DATA/FINAL`).
        #[arg(long)]
        game_dir: Utf8PathBuf,
        /// Overlay output directory (mirrors `DATA/FINAL`).
        #[arg(short, long)]
        output: Utf8PathBuf,
        /// Directory for builder state (defaults to `<output>.state`).
        #[arg(long)]
        state_dir: Option<Utf8PathBuf>,
        /// Mod archives (`.fantome`) to enable, in priority order
        /// (first wins on conflicts).
        #[arg(required = true)]
        mods: Vec<Utf8PathBuf>,
    },
    /// Pack roots, set files, and statements into a single bundle blob —
    /// everything a verifier needs besides its baked platform key.
    BuildBundle {
        /// Signed root token. Repeatable.
        #[arg(long = "root", required = true)]
        roots: Vec<Utf8PathBuf>,
        /// Set file (named by the root, so plain paths). Repeatable.
        #[arg(long = "set", required = true)]
        sets: Vec<Utf8PathBuf>,
        /// Directory containing `.token` statement files.
        #[arg(long)]
        statements: Option<Utf8PathBuf>,
        /// Path to write the bundle to.
        #[arg(short, long)]
        output: Utf8PathBuf,
    },
    /// Verify the base skins of every champion WAD in an overlay against
    /// the original game WADs, using a bundle (roots + sets + statements)
    /// as the game-side verifier would.
    Verify {
        /// League `Game` directory (contains `DATA/FINAL`).
        #[arg(long)]
        game_dir: Utf8PathBuf,
        /// Overlay directory to verify (as produced by `build-overlay`).
        #[arg(long)]
        overlay: Utf8PathBuf,
        /// Bundle blob (as produced by `build-bundle`).
        #[arg(long)]
        bundle: Utf8PathBuf,
        /// Platform public key as 64 hex chars.
        #[arg(long, conflicts_with = "key")]
        pub_key: Option<String>,
        /// Platform private key seed file (public key is derived; dev
        /// convenience only).
        #[arg(long)]
        key: Option<Utf8PathBuf>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    match Args::parse().command {
        Commands::Keygen { output } => commands::keygen::run(&output),
        Commands::Pubkey { key } => commands::pubkey::run(&key),
        Commands::Extract {
            game_dir,
            output,
            mods_dir,
            mods,
        } => commands::extract::run(&game_dir, &output, mods_dir.as_deref(), &mods),
        Commands::BuildSet { statements, output } => commands::build_set::run(&statements, &output),
        Commands::SignRoot {
            key,
            sets,
            predecessor,
            output,
        } => commands::sign_root::run(&key, &sets, predecessor.as_deref(), &output),
        Commands::BuildOverlay {
            game_dir,
            output,
            state_dir,
            mods,
        } => commands::build_overlay::run(&game_dir, &output, state_dir.as_deref(), &mods),
        Commands::BuildBundle {
            roots,
            sets,
            statements,
            output,
        } => commands::build_bundle::run(&roots, &sets, statements.as_deref(), &output),
        Commands::Verify {
            game_dir,
            overlay,
            bundle,
            pub_key,
            key,
        } => commands::verify::run(&commands::verify::VerifyArgs {
            game_dir,
            overlay,
            bundle,
            pub_key,
            key,
        }),
    }
}
