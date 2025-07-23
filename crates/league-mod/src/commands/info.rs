use std::fs::File;

use league_modpkg::Modpkg;

pub struct InfoModPackageArgs {
    pub file_path: String,
}

pub fn info_mod_package(args: InfoModPackageArgs) -> eyre::Result<()> {
    let file = File::open(&args.file_path)?;
    let modpkg = Modpkg::mount_from_reader(file)?;

    println!("Modpkg: {}", modpkg.metadata.name);
    println!("Version: {}", modpkg.metadata.version);
    println!(
        "Description: {}",
        modpkg
            .metadata
            .description
            .unwrap_or("No description".to_string())
    );

    println!("Layers:");
    for layer in modpkg.layers.values() {
        println!("  {}", layer.name);
    }

    println!("Chunks:");
    for chunk in modpkg.chunks.values() {
        println!("  {}", modpkg.chunk_paths[&chunk.path_hash]);
    }

    Ok(())
}
