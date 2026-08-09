//! Built-in CottSynth VST3 discovery and catalog injection.
//!
//! The plugin always appears in the browser. MIDI tracks load it via the normal
//! sandboxed worker path (same as any other VST3).

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Stable catalog UID (VST3 CID bytes for `CottSynthVST3CE!` as hex).
pub const COTT_SYNTH_UID: &str = "436F747453796E746856535433434521";
pub const COTT_SYNTH_NAME: &str = "CottSynth";
pub const COTT_SYNTH_VENDOR: &str = "Cottage";

/// Locate the bundled `cott-synth.vst3` next to the DAW / in the workspace build tree.
pub fn resolve_cott_synth_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = option_env!("COTT_SYNTH_VST3") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("plugins/cott-synth.vst3"));
        candidates.push(dir.join("cott-synth.vst3"));
    }

    // Workspace-relative paths (cargo run from repo root).
    candidates.push(PathBuf::from("target/bundled/cott-synth.vst3"));
    candidates.push(PathBuf::from("target/debug/plugins/cott-synth.vst3"));
    candidates.push(PathBuf::from("target/release/plugins/cott-synth.vst3"));

    // Walk up from CWD looking for the Cargo workspace + bundled artifact.
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        for _ in 0..6 {
            candidates.push(dir.join("target/bundled/cott-synth.vst3"));
            if dir.join("Cargo.toml").is_file() && dir.join("crates/cott-synth").is_dir() {
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
                .map(|mut d| d.any(|e| e.map(|e| e.path().extension() == Some("so".as_ref())).unwrap_or(false)))
                .unwrap_or(false))
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Descriptor for the browser — present even if the scan finds nothing else.
pub fn cott_synth_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_SYNTH_UID.into(),
        name: COTT_SYNTH_NAME.into(),
        vendor: COTT_SYNTH_VENDOR.into(),
        path,
        is_instrument: true,
        is_effect: false,
        has_editor: true,
    }
}

/// Ensure CottSynth is first in the catalog when the bundle is available.
pub fn inject_cott_synth(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_synth_vst3() else {
        warn!(
            "CottSynth.vst3 not found — run `cargo bundle-synth` (or rebuild cott-daw) to bake it in"
        );
        // Still show a stub entry so the browser always lists it; load will explain.
        let stub = cott_synth_descriptor(PathBuf::from(
            "target/bundled/cott-synth.vst3 (missing — run cargo bundle-synth)",
        ));
        catalog.retain(|p| p.uid != COTT_SYNTH_UID && p.name != COTT_SYNTH_NAME);
        catalog.insert(0, stub);
        return;
    };

    let desc = cott_synth_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_SYNTH_UID
            && p.name != COTT_SYNTH_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottSynth at {}", path.display());
    catalog.insert(0, desc);
}

