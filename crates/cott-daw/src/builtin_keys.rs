//! Built-in CottKeys VST3 discovery and catalog injection.

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Stable catalog UID (VST3 CID bytes for `CottKeysVST3CE!!` as hex).
pub const COTT_KEYS_UID: &str = "436F74744B6579735653543343452121";
pub const COTT_KEYS_NAME: &str = "CottKeys";
pub const COTT_KEYS_VENDOR: &str = "Cottage";

pub fn resolve_cott_keys_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = option_env!("COTT_KEYS_VST3") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("plugins/cott-keys.vst3"));
        candidates.push(dir.join("cott-keys.vst3"));
    }

    candidates.push(PathBuf::from("target/bundled/cott-keys.vst3"));
    candidates.push(PathBuf::from("target/debug/plugins/cott-keys.vst3"));
    candidates.push(PathBuf::from("target/release/plugins/cott-keys.vst3"));

    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        for _ in 0..6 {
            candidates.push(dir.join("target/bundled/cott-keys.vst3"));
            if dir.join("Cargo.toml").is_file() && dir.join("crates/cott-keys").is_dir() {
                break;
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }

    for path in candidates {
        if bundle_looks_valid(&path) {
            return Some(canonicalize_or_self(&path));
        }
    }
    None
}

fn bundle_looks_valid(path: &Path) -> bool {
    path.is_dir()
        && (path.join("Contents").is_dir()
            || path
                .read_dir()
                .map(|mut d| {
                    d.any(|e| {
                        e.map(|e| e.path().extension() == Some("so".as_ref()))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false))
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn cott_keys_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_KEYS_UID.into(),
        name: COTT_KEYS_NAME.into(),
        vendor: COTT_KEYS_VENDOR.into(),
        path,
        is_instrument: true,
        is_effect: false,
        has_editor: true,
    }
}

pub fn inject_cott_keys(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_keys_vst3() else {
        warn!(
            "CottKeys.vst3 not found — run `cargo bundle-keys` (or rebuild with build-daw) to bake it in"
        );
        let stub = cott_keys_descriptor(PathBuf::from(
            "target/bundled/cott-keys.vst3 (missing — run cargo bundle-keys)",
        ));
        catalog.retain(|p| p.uid != COTT_KEYS_UID && p.name != COTT_KEYS_NAME);
        catalog.insert(insert_position(catalog), stub);
        return;
    };

    let desc = cott_keys_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_KEYS_UID
            && p.name != COTT_KEYS_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottKeys at {}", path.display());
    catalog.insert(insert_position(catalog), desc);
}

fn insert_position(catalog: &[PluginDescriptor]) -> usize {
    catalog
        .iter()
        .position(|p| p.name == crate::builtin_bass::COTT_BASS_NAME)
        .or_else(|| {
            catalog
                .iter()
                .position(|p| p.name == crate::builtin_drums::COTT_DRUMS_NAME)
        })
        .map(|i| i + 1)
        .unwrap_or(0)
}
