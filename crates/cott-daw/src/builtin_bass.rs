//! Built-in CottBass VST3 discovery and catalog injection.

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub const COTT_BASS_UID: &str = "436F7474426173735653543343452121";
pub const COTT_BASS_NAME: &str = "CottBass";
pub const COTT_BASS_VENDOR: &str = "Cottage";

pub fn resolve_cott_bass_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = option_env!("COTT_BASS_VST3") {
        candidates.push(PathBuf::from(path));
    }
    push_common(&mut candidates, "cott-bass");
    candidates
        .into_iter()
        .find(|p| bundle_looks_valid(p))
        .map(|p| canonicalize_or_self(&p))
}

fn push_common(candidates: &mut Vec<PathBuf>, stem: &str) {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join(format!("plugins/{stem}.vst3")));
        candidates.push(dir.join(format!("{stem}.vst3")));
    }
    candidates.push(PathBuf::from(format!("target/bundled/{stem}.vst3")));
    candidates.push(PathBuf::from(format!("target/debug/plugins/{stem}.vst3")));
    candidates.push(PathBuf::from(format!("target/release/plugins/{stem}.vst3")));
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        for _ in 0..6 {
            candidates.push(dir.join(format!("target/bundled/{stem}.vst3")));
            if dir.join("Cargo.toml").is_file() && dir.join(format!("crates/{stem}")).is_dir() {
                break;
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }
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

pub fn cott_bass_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_BASS_UID.into(),
        name: COTT_BASS_NAME.into(),
        vendor: COTT_BASS_VENDOR.into(),
        path,
        is_instrument: true,
        is_effect: false,
        has_editor: true,
    }
}

pub fn inject_cott_bass(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_bass_vst3() else {
        warn!(
            "CottBass.vst3 not found — run `cargo bundle-bass` (or rebuild with build-daw) to bake it in"
        );
        let stub = cott_bass_descriptor(PathBuf::from(
            "target/bundled/cott-bass.vst3 (missing — run cargo bundle-bass)",
        ));
        catalog.retain(|p| p.uid != COTT_BASS_UID && p.name != COTT_BASS_NAME);
        catalog.insert(insert_position(catalog), stub);
        return;
    };
    let desc = cott_bass_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_BASS_UID
            && p.name != COTT_BASS_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottBass at {}", path.display());
    catalog.insert(insert_position(catalog), desc);
}

fn insert_position(catalog: &[PluginDescriptor]) -> usize {
    catalog
        .iter()
        .position(|p| p.name == crate::builtin_tape::COTT_TAPE_NAME)
        .or_else(|| catalog.iter().position(|p| p.name == crate::builtin_vinyl::COTT_VINYL_NAME))
        .map(|i| i + 1)
        .unwrap_or(catalog.len())
}
