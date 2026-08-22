//! Built-in CottHaze VST3 discovery and catalog injection.
//!
//! Always listed in the browser as an instrument. Loaded via the sandboxed
//! worker, exactly like CottSynth and CottWhistle.

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Stable catalog UID (VST3 CID bytes for `CottHazeVST3CE!!` as hex).
pub const COTT_HAZE_UID: &str = "436F747448617A655653543343452121";
pub const COTT_HAZE_NAME: &str = "CottHaze";
pub const COTT_HAZE_VENDOR: &str = "Cottage";

/// Locate the bundled `cott-haze.vst3` next to the DAW / in the build tree.
pub fn resolve_cott_haze_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = option_env!("COTT_HAZE_VST3") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("plugins/cott-haze.vst3"));
        candidates.push(dir.join("cott-haze.vst3"));
    }

    candidates.push(PathBuf::from("target/bundled/cott-haze.vst3"));
    candidates.push(PathBuf::from("target/debug/plugins/cott-haze.vst3"));
    candidates.push(PathBuf::from("target/release/plugins/cott-haze.vst3"));

    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        for _ in 0..6 {
            candidates.push(dir.join("target/bundled/cott-haze.vst3"));
            if dir.join("Cargo.toml").is_file() && dir.join("crates/cott-haze").is_dir() {
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

pub fn cott_haze_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_HAZE_UID.into(),
        name: COTT_HAZE_NAME.into(),
        vendor: COTT_HAZE_VENDOR.into(),
        path,
        is_instrument: true,
        is_effect: false,
        has_editor: true,
    }
}

/// Ensure CottHaze sits with the other first-party plugins at the top.
pub fn inject_cott_haze(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_haze_vst3() else {
        warn!(
            "CottHaze.vst3 not found — run `cargo bundle-haze` (or rebuild with build-daw) to bake it in"
        );
        let stub = cott_haze_descriptor(PathBuf::from(
            "target/bundled/cott-haze.vst3 (missing — run cargo bundle-haze)",
        ));
        catalog.retain(|p| p.uid != COTT_HAZE_UID && p.name != COTT_HAZE_NAME);
        catalog.insert(insert_position(catalog), stub);
        return;
    };

    let desc = cott_haze_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_HAZE_UID
            && p.name != COTT_HAZE_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottHaze at {}", path.display());
    catalog.insert(insert_position(catalog), desc);
}

/// Right after CottWhistle so the Cottage plugins stay grouped.
fn insert_position(catalog: &[PluginDescriptor]) -> usize {
    catalog
        .iter()
        .position(|p| p.name == crate::builtin_whistle::COTT_WHISTLE_NAME)
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
