use league_modpkg::{ModpkgAuthor, ModpkgLicense};
use mod_project::{ModProjectAuthor, ModProjectLicense};

pub fn convert_project_author(author: &ModProjectAuthor) -> ModpkgAuthor {
    match author {
        ModProjectAuthor::Name(name) => ModpkgAuthor {
            name: name.clone(),
            role: None,
        },
        ModProjectAuthor::Role { name, role } => ModpkgAuthor {
            name: name.clone(),
            role: Some(role.clone()),
        },
    }
}

pub fn convert_project_license(license: &Option<ModProjectLicense>) -> ModpkgLicense {
    match license {
        None => ModpkgLicense::None,
        Some(ModProjectLicense::Spdx(spdx_id)) => ModpkgLicense::Spdx {
            spdx_id: spdx_id.clone(),
        },
        Some(ModProjectLicense::Custom { name, url }) => ModpkgLicense::Custom {
            name: name.clone(),
            url: url.clone(),
        },
    }
}
