//! Built-in CottDrums VST3 discovery and catalog injection.

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Stable catalog UID (VST3 CID bytes for `CottDrumsVST3CE!` as hex).
pub const COTT_DRUMS_UID: &str = "436F74744472756D7356535433434521";
pub const COTT_DRUMS_NAME: &str = "CottDrums";
pub const COTT_DRUMS_VENDOR: &str = "Cottage";

pub fn resolve_cott_drums_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = option_env!("COTT_DRUMS_VST3") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("plugins/cott-drums.vst3"));
        candidates.push(dir.join("cott-drums.vst3"));
    }

    candidates.push(PathBuf::from("target/bundled/cott-drums.vst3"));
    candidates.push(PathBuf::from("target/debug/plugins/cott-drums.vst3"));
    candidates.push(PathBuf::from("target/release/plugins/cott-drums.vst3"));

    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        for _ in 0..6 {
            candidates.push(dir.join("target/bundled/cott-drums.vst3"));
            if dir.join("Cargo.toml").is_file() && dir.join("crates/cott-drums").is_dir() {
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

pub fn cott_drums_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_DRUMS_UID.into(),
        name: COTT_DRUMS_NAME.into(),
        vendor: COTT_DRUMS_VENDOR.into(),
        path,
        is_instrument: true,
        is_effect: false,
        has_editor: true,
    }
}

pub fn inject_cott_drums(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_drums_vst3() else {
        warn!(
            "CottDrums.vst3 not found — run `cargo bundle-drums` (or rebuild with build-daw) to bake it in"
        );
        let stub = cott_drums_descriptor(PathBuf::from(
            "target/bundled/cott-drums.vst3 (missing — run cargo bundle-drums)",
        ));
        catalog.retain(|p| p.uid != COTT_DRUMS_UID && p.name != COTT_DRUMS_NAME);
        catalog.insert(insert_position(catalog), stub);
        return;
    };

    let desc = cott_drums_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_DRUMS_UID
            && p.name != COTT_DRUMS_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottDrums at {}", path.display());
    catalog.insert(insert_position(catalog), desc);
}

fn insert_position(catalog: &[PluginDescriptor]) -> usize {
    catalog
        .iter()
        .position(|p| p.name == crate::builtin_vinyl::COTT_VINYL_NAME)
        .or_else(|| {
            catalog
                .iter()
                .position(|p| p.name == crate::builtin_filter::COTT_FILTER_NAME)
        })
        .or_else(|| {
            catalog
                .iter()
                .position(|p| p.name == crate::builtin_synth::COTT_SYNTH_NAME)
        })
        .map(|i| i + 1)
        .unwrap_or(0)
}
