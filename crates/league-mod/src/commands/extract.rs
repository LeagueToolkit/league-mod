use std::fs::File;
use std::path::Path;

use league_modpkg::{Modpkg, ModpkgExtractor};

pub struct ExtractModPackageArgs {
    pub file_path: String,
    pub output_dir: String,
}

pub fn extract_mod_package(args: ExtractModPackageArgs) -> eyre::Result<()> {
    let file = File::open(&args.file_path)?;
    let mut modpkg = Modpkg::mount_from_reader(file)?;

    println!("Extracting modpkg: {}", args.file_path);

    let output_path = Path::new(&args.output_dir);
    let mut extractor = ModpkgExtractor::new(&mut modpkg);

    println!("Extracting to: {}", output_path.display());
    extractor.extract_all(output_path)?;

    println!("Extraction complete!");

    Ok(())
}
