//! Built-in CottFilter VST3 discovery and catalog injection.
//!
//! Always listed in the browser as an effect. Loaded via the sandboxed worker.

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Stable catalog UID (VST3 CID bytes for `CottFiltVST3CE!!` as hex).
pub const COTT_FILTER_UID: &str = "436F747446696C745653543343452121";
pub const COTT_FILTER_NAME: &str = "CottFilter";
pub const COTT_FILTER_VENDOR: &str = "Cottage";

/// Locate the bundled `cott-filter.vst3` next to the DAW / in the workspace build tree.
pub fn resolve_cott_filter_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = option_env!("COTT_FILTER_VST3") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("plugins/cott-filter.vst3"));
        candidates.push(dir.join("cott-filter.vst3"));
    }

    candidates.push(PathBuf::from("target/bundled/cott-filter.vst3"));
    candidates.push(PathBuf::from("target/debug/plugins/cott-filter.vst3"));
    candidates.push(PathBuf::from("target/release/plugins/cott-filter.vst3"));

    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        for _ in 0..6 {
            candidates.push(dir.join("target/bundled/cott-filter.vst3"));
            if dir.join("Cargo.toml").is_file() && dir.join("crates/cott-filter").is_dir() {
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

pub fn cott_filter_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_FILTER_UID.into(),
        name: COTT_FILTER_NAME.into(),
        vendor: COTT_FILTER_VENDOR.into(),
        path,
        is_instrument: false,
        is_effect: true,
        has_editor: true,
    }
}

/// Ensure CottFilter is in the catalog (right after CottSynth when both exist).
pub fn inject_cott_filter(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_filter_vst3() else {
        warn!(
            "CottFilter.vst3 not found — run `cargo bundle-filter` (or rebuild with build-daw) to bake it in"
        );
        let stub = cott_filter_descriptor(PathBuf::from(
            "target/bundled/cott-filter.vst3 (missing — run cargo bundle-filter)",
        ));
        catalog.retain(|p| p.uid != COTT_FILTER_UID && p.name != COTT_FILTER_NAME);
        let insert_at = catalog
            .iter()
            .position(|p| p.name == "CottSynth")
            .map(|i| i + 1)
            .unwrap_or(0);
        catalog.insert(insert_at, stub);
        return;
    };

    let desc = cott_filter_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_FILTER_UID
            && p.name != COTT_FILTER_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottFilter at {}", path.display());
    let insert_at = catalog
        .iter()
        .position(|p| p.name == "CottSynth")
        .map(|i| i + 1)
        .unwrap_or(0);
    catalog.insert(insert_at, desc);
}
