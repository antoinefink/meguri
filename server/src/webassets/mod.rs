// SPDX-License-Identifier: MIT
//! Exact embedded browser assets. The caller owns HTTP, HTTPS/origin policy and
//! security headers. Only GET/HEAD requests without a query may use this lookup.
//! Generate OUT_DIR/meguri_web_assets.rs with server/web/scripts/embed.mjs,
//! then copy it and its embedded/ sibling into OUT_DIR in the server build script.
#[derive(Debug)]
pub struct Asset {
    pub path: &'static str,
    pub content_type: &'static str,
    pub bytes: &'static [u8],
}

pub const NAVIGATION: &[&str] = &[
    "/", "/setup", "/login", "/invite", "/reset", "/connection", "/settings", "/accounts",
];

include!(concat!(env!("OUT_DIR"), "/meguri_web_assets.rs"));

/// No URL decoding, filesystem access, path normalization or shell fallback.
/// Pass the original URI path, not a decoded or normalized path.
pub fn lookup(path: &str) -> Option<&'static Asset> {
    select(path, &INDEX, ASSETS)
}

fn select(path: &str, index: &'static Asset, assets: &'static [Asset]) -> Option<&'static Asset> {
    if NAVIGATION.contains(&path) {
        return Some(index);
    }
    if !hashed_asset_path(path) {
        return None;
    }
    assets.iter().find(|asset| asset.path == path)
}

fn hashed_asset_path(path: &str) -> bool {
    let Some(name) = path.strip_prefix("/assets/") else { return false };
    if !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        || name.contains("..")
    {
        return false;
    }
    let Some((stem, extension)) = name.rsplit_once('.') else { return false };
    if !matches!(extension, "js" | "css" | "png" | "woff2") {
        return false;
    }
    // Vite hashes may themselves contain '-' or '_'. Find a separator leaving
    // at least eight base64url characters, rather than taking the final '-'.
    stem.match_indices('-').any(|(position, _)| {
        let hash = &stem[position + 1..];
        position > 0 && hash.len() >= 8
            && hash.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    static SHELL: Asset = Asset { path: "/index.html", content_type: "text/html; charset=utf-8", bytes: b"shell" };
    static FILES: &[Asset] = &[
        Asset { path: "/assets/main-Ab12_-34.js", content_type: "text/javascript; charset=utf-8", bytes: b"script" },
        Asset { path: "/assets/main-Ab123456.css", content_type: "text/css; charset=utf-8", bytes: b"style" },
        Asset { path: "/assets/favicon-Ab123456.png", content_type: "image/png", bytes: b"icon" },
        Asset { path: "/assets/IBMPlexSans-Variable-Ab123456.woff2", content_type: "font/woff2", bytes: b"font" },
    ];
    #[test]
    fn every_navigation_path_and_only_those_paths_gets_shell() {
        for path in NAVIGATION {
            assert!(std::ptr::eq(select(path, &SHELL, FILES).unwrap(), &SHELL));
        }
        for path in ["/index.html", "/accounts/", "/unknown", "/api/status", "/api/unknown", "/sync/", "/msync/", "/health"] {
            assert!(select(path, &SHELL, FILES).is_none());
        }
    }
    #[test]
    fn assets_require_both_hashed_local_name_and_exact_manifest_membership() {
        for asset in FILES {
            let found = select(asset.path, &SHELL, FILES).unwrap();
            assert_eq!(found.bytes, asset.bytes);
            assert_eq!(found.content_type, asset.content_type);
        }
        for path in ["/assets/missing-Ab123456.js", "/assets/missing-Ab123456.woff2", "/assets/IBMPlexSans-Variable.woff2", "/assets/main.js", "/assets/main-Ab123456.js.map", "/assets/favicon-Ab123456.svg", "/assets/favicon.png", "/assets/../login", "/assets/%2e%2e/login", "/assets/main-Ab123456.css?x", "/assets//main-Ab123456.css", "/assets/evil\\main-Ab123456.js", "/login#token=x", "/login?x", "/assets/main-1234567.js", "//login"] {
            assert!(select(path, &SHELL, FILES).is_none());
        }
    }
    #[test]
    fn generated_manifest_is_bounded_and_contains_only_local_bundles() {
        assert_eq!(INDEX.content_type, "text/html; charset=utf-8");
        assert!(!INDEX.bytes.is_empty());
        assert!(!ASSETS.is_empty());
        for (position, asset) in ASSETS.iter().enumerate() {
            assert!(hashed_asset_path(asset.path));
            assert!(!asset.bytes.is_empty());
            assert!(ASSETS[..position].iter().all(|earlier| earlier.path != asset.path));
        }
    }
}
