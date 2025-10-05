use camino::{Utf8Path, Utf8PathBuf};
use glob::Pattern;
use ltk_mod_project::{FileTransformer, ModProject};
use std::collections::{HashMap, HashSet};

/// Planned transforms for a single layer.
#[derive(Debug, Default, Clone)]
pub struct LayerTransformPlan {
    /// Files that should be excluded from packing because they are inputs to a transformer.
    pub excluded_inputs: HashSet<Utf8PathBuf>,
    /// Files that should be additionally included (expected outputs) produced by transformers.
    /// For now we only plan expected outputs; generation is handled elsewhere in the future.
    pub expected_outputs: HashSet<Utf8PathBuf>,
}

/// Full plan across layers: layer_name -> plan
pub type TransformPlan = HashMap<String, LayerTransformPlan>;

/// Compute which files should be excluded or included based on configured transformers.
/// This is a minimal, non-executing planning stage.
pub fn plan_transforms(mod_project: &ModProject, content_dir: &Utf8Path) -> TransformPlan {
    let mut plan: TransformPlan = HashMap::new();

    if mod_project.transformers.is_empty() {
        return plan;
    }

    // Pre-compile glob patterns per transformer
    let compiled: Vec<(String, Vec<Pattern>)> = mod_project
        .transformers
        .iter()
        .map(|t: &FileTransformer| {
            let pats = t
                .patterns
                .iter()
                .filter_map(|p| Pattern::new(p).ok())
                .collect::<Vec<_>>();
            (t.name.clone(), pats)
        })
        .collect();

    // For each layer, walk files and mark matches
    for layer in &mod_project.layers {
        let layer_dir = content_dir.join(layer.name.as_str());
        let mut layer_plan = LayerTransformPlan::default();

        // Gather files with a naive glob walk
        if let Ok(paths) = glob::glob(layer_dir.join("**/*").as_str()) {
            for entry in paths.flatten() {
                if !entry.is_file() {
                    continue;
                }

                let entry = Utf8Path::from_path(&entry).expect("entry must be valid UTF-8");

                // Get relative path within layer for pattern matching
                let rel = match entry.strip_prefix(&layer_dir) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                // Check each transformer
                for (name, patterns) in &compiled {
                    if patterns.iter().any(|p| p.matches(rel.as_str())) {
                        // Currently we only have awareness of "tex-converter"
                        if name == "tex-converter" {
                            layer_plan.excluded_inputs.insert(entry.to_path_buf());

                            // Compute expected output path by swapping extension to .tex
                            let mut out_rel = rel.to_path_buf();
                            out_rel.set_extension("tex");
                            let out_abs = layer_dir.join(out_rel);
                            layer_plan.expected_outputs.insert(out_abs);
                        }
                    }
                }
            }
        }

        plan.insert(layer.name.clone(), layer_plan);
    }

    // Ensure base layer exists in plan as well, even if not explicitly in config
    if !plan.contains_key("base") {
        let layer_dir = content_dir.join("base");
        if layer_dir.exists() {
            plan.insert("base".to_string(), LayerTransformPlan::default());
        }
    }

    plan
}
