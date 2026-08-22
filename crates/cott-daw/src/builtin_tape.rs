//! Built-in CottTape VST3 discovery and catalog injection.

use cott_ipc::{PluginDescriptor, PluginFormat};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub const COTT_TAPE_UID: &str = "436F7474546170655653543343452121";
pub const COTT_TAPE_NAME: &str = "CottTape";
pub const COTT_TAPE_VENDOR: &str = "Cottage";

pub fn resolve_cott_tape_vst3() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = option_env!("COTT_TAPE_VST3") {
        candidates.push(PathBuf::from(path));
    }
    let stem = "cott-tape";
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

pub fn cott_tape_descriptor(path: PathBuf) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Vst3,
        uid: COTT_TAPE_UID.into(),
        name: COTT_TAPE_NAME.into(),
        vendor: COTT_TAPE_VENDOR.into(),
        path,
        is_instrument: false,
        is_effect: true,
        has_editor: true,
    }
}

pub fn inject_cott_tape(catalog: &mut Vec<PluginDescriptor>) {
    let Some(path) = resolve_cott_tape_vst3() else {
        warn!(
            "CottTape.vst3 not found — run `cargo bundle-tape` (or rebuild with build-daw) to bake it in"
        );
        let stub = cott_tape_descriptor(PathBuf::from(
            "target/bundled/cott-tape.vst3 (missing — run cargo bundle-tape)",
        ));
        catalog.retain(|p| p.uid != COTT_TAPE_UID && p.name != COTT_TAPE_NAME);
        catalog.insert(insert_position(catalog), stub);
        return;
    };
    let desc = cott_tape_descriptor(path.clone());
    catalog.retain(|p| {
        p.uid != COTT_TAPE_UID
            && p.name != COTT_TAPE_NAME
            && canonicalize_or_self(&p.path) != path
    });
    info!("baked-in CottTape at {}", path.display());
    catalog.insert(insert_position(catalog), desc);
}

fn insert_position(catalog: &[PluginDescriptor]) -> usize {
    catalog
        .iter()
        .position(|p| p.name == crate::builtin_vinyl::COTT_VINYL_NAME)
        .or_else(|| catalog.iter().position(|p| p.name == crate::builtin_haze::COTT_HAZE_NAME))
        .map(|i| i + 1)
        .unwrap_or(catalog.len())
}
