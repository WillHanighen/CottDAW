//! Built-in CottPluck VST3 discovery and catalog injection.

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub const COTT_PLUCK_UID: &str = "436F7474506C75636B56535433434521";
pub const COTT_PLUCK_NAME: &str = "CottPluck";
pub const COTT_PLUCK_VENDOR: &str = "Cottage";

pub fn resolve_cott_pluck_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = option_env!("COTT_PLUCK_VST3") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("plugins/cott-pluck.vst3"));
        candidates.push(dir.join("cott-pluck.vst3"));
    }
    candidates.push(PathBuf::from("target/bundled/cott-pluck.vst3"));
    candidates.push(PathBuf::from("target/debug/plugins/cott-pluck.vst3"));
    candidates.push(PathBuf::from("target/release/plugins/cott-pluck.vst3"));
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        for _ in 0..6 {
            candidates.push(dir.join("target/bundled/cott-pluck.vst3"));
            if dir.join("Cargo.toml").is_file() && dir.join("crates/cott-pluck").is_dir() {
                break;
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }
    candidates
        .into_iter()
        .find(|p| bundle_looks_valid(p))
        .map(|p| canonicalize_or_self(&p))
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

pub fn cott_pluck_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_PLUCK_UID.into(),
        name: COTT_PLUCK_NAME.into(),
        vendor: COTT_PLUCK_VENDOR.into(),
        path,
        is_instrument: true,
        is_effect: false,
        has_editor: true,
    }
}

pub fn inject_cott_pluck(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_pluck_vst3() else {
        warn!(
            "CottPluck.vst3 not found — run `cargo bundle-pluck` (or rebuild with build-daw) to bake it in"
        );
        let stub = cott_pluck_descriptor(PathBuf::from(
            "target/bundled/cott-pluck.vst3 (missing — run cargo bundle-pluck)",
        ));
        catalog.retain(|p| p.uid != COTT_PLUCK_UID && p.name != COTT_PLUCK_NAME);
        catalog.insert(insert_position(catalog), stub);
        return;
    };
    let desc = cott_pluck_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_PLUCK_UID
            && p.name != COTT_PLUCK_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottPluck at {}", path.display());
    catalog.insert(insert_position(catalog), desc);
}

fn insert_position(catalog: &[PluginDescriptor]) -> usize {
    catalog
        .iter()
        .position(|p| p.name == crate::builtin_bass::COTT_BASS_NAME)
        .or_else(|| catalog.iter().position(|p| p.name == crate::builtin_tape::COTT_TAPE_NAME))
        .map(|i| i + 1)
        .unwrap_or(catalog.len())
}
