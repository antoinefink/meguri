//! P18: which flow a file belongs to, by its name first and its content
//! otherwise. Pure: it reads at most the archive's directory.
use std::path::Path;

/// `package/container` P18's classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageClass {
    /// A deck package: `package/apkg-import`.
    Apkg,
    /// A whole collection: the collection package import.
    Colpkg,
    Unsupported,
}

/// Step 1 alone: the class a name gives, or `None` when the content decides.
pub fn classify_name(name: &str) -> Option<PackageClass> {
    let name = name.to_lowercase();
    if name.ends_with(".colpkg") || name == "collection.apkg" || (name.starts_with("backup-") && name.ends_with(".apkg"))
    {
        Some(PackageClass::Colpkg)
    } else if name.ends_with(".apkg") || name.ends_with(".zip") {
        Some(PackageClass::Apkg)
    } else {
        None
    }
}

/// P18 for the file at `path`, read in place and read only. `name` is the
/// name the user saw (a provider may hand over another path).
pub fn classify(path: &Path, name: &str) -> PackageClass {
    if let Some(class) = classify_name(name) {
        return class;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return PackageClass::Unsupported;
    };
    match crate::inspection_zip::open_with_legacy(file, &|| false) {
        Ok(found)
            if found.meta.is_some()
                || found.collection.is_some()
                || found.legacy21.is_some()
                || found.legacy2.is_some() =>
        {
            PackageClass::Apkg
        }
        _ => PackageClass::Unsupported,
    }
}
