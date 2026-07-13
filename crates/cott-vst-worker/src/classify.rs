//! Enrich VST3 scan metadata — truce-rack currently hardcodes
//! `category = Effect` and `accepts_midi = false` for every plugin.

use std::path::Path;

use tracing::debug;
use vst3::ComPtr;
use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo_,
    PClassInfo2, PClassInfo2_, TUID, kResultOk,
};

/// Returns true when the VST3 class subcategories indicate an instrument.
pub fn bundle_is_instrument(bundle: &Path, uid_hex: &str) -> Option<bool> {
    let binary = bundle_binary_path(bundle);
    if !binary.exists() {
        return None;
    }
    let library = unsafe { libloading::Library::new(&binary) }.ok()?;

    // Linux VST3 / yabridge: ModuleEntry before GetPluginFactory.
    let mut entered = false;
    if let Ok(entry) = unsafe {
        library.get::<unsafe extern "C" fn(*mut std::ffi::c_void) -> bool>(b"ModuleEntry\0")
    } {
        if !unsafe { entry(std::ptr::null_mut()) } {
            return None;
        }
        entered = true;
    }

    let result = (|| {
        let get_factory: libloading::Symbol<'_, unsafe extern "C" fn() -> *mut IPluginFactory> =
            unsafe { library.get(b"GetPluginFactory\0") }.ok()?;
        let factory_ptr = unsafe { get_factory() };
        let factory = unsafe { ComPtr::<IPluginFactory>::from_raw(factory_ptr) }?;
        let factory2: ComPtr<IPluginFactory2> = factory.cast()?;
        let target = hex_to_tuid(uid_hex)?;

        let count = unsafe { factory2.countClasses() };
        for idx in 0..count {
            let mut info = empty_pclass_info2();
            if unsafe { factory2.getClassInfo2(idx, &mut info) } != kResultOk {
                continue;
            }
            if info.cid != target {
                continue;
            }
            let category = c_array_to_string(&info.category);
            if category != "Audio Module Class" {
                return Some(false);
            }
            let subs = c_array_to_string(&info.subCategories).to_ascii_lowercase();
            let is_instrument = subs.split('|').any(|part| {
                matches!(
                    part.trim(),
                    "instrument" | "synth" | "sampler" | "drum" | "piano"
                ) || part.contains("instrument")
            });
            debug!(
                plugin = %c_array_to_string(&info.name),
                subcategories = %subs,
                is_instrument,
                "classified VST3"
            );
            return Some(is_instrument);
        }
        None
    })();

    if entered {
        if let Ok(exit) = unsafe { library.get::<unsafe extern "C" fn() -> bool>(b"ModuleExit\0") }
        {
            unsafe {
                let _ = exit();
            }
        }
    }

    result
}

/// Name-based fallback when factory subcategory probing fails.
pub fn name_looks_like_instrument(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "synth",
        "sampler",
        "instrument",
        "piano",
        "organ",
        "drum",
        "keys",
        "bass",
        "lead",
        "pad",
        "nya",
        "pluck",
        "vox",
    ]
    .iter()
    .any(|needle| n.contains(needle))
}

/// Conservative effect-name hints (used when skipping factory probes).
pub fn name_looks_like_effect(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "eq", "comp", "reverb", "delay", "limiter", "gate", "filter", "mix link", "utility",
        "analyzer", "saturat", "distort", "chorus", "flanger", "phaser",
    ]
    .iter()
    .any(|needle| n.contains(needle))
}

fn bundle_binary_path(bundle: &Path) -> std::path::PathBuf {
    let stem = bundle
        .file_stem()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    if bundle.is_dir() {
        let arch_dir = format!("{}-linux", std::env::consts::ARCH);
        let mut binary = stem;
        binary.push(".so");
        return bundle.join("Contents").join(arch_dir).join(binary);
    }
    bundle.to_path_buf()
}

fn empty_pclass_info2() -> PClassInfo2 {
    PClassInfo2 {
        cid: [0; 16],
        cardinality: 0,
        category: [0; PClassInfo_::kCategorySize as usize],
        name: [0; PClassInfo_::kNameSize as usize],
        classFlags: 0,
        subCategories: [0; PClassInfo2_::kSubCategoriesSize as usize],
        vendor: [0; PClassInfo2_::kVendorSize as usize],
        version: [0; PClassInfo2_::kVersionSize as usize],
        sdkVersion: [0; PClassInfo2_::kVersionSize as usize],
    }
}

fn c_array_to_string(array: &[i8]) -> String {
    let bytes: Vec<u8> = array
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn hex_to_tuid(hex: &str) -> Option<TUID> {
    if hex.len() != 32 {
        return None;
    }
    let mut out: TUID = [0; 16];
    for i in 0..16 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()? as i8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classifies_nyasynth_as_instrument() {
        let path = PathBuf::from(std::env::var("HOME").unwrap()).join(".vst3/nyasynth.vst3");
        if !path.exists() {
            eprintln!("skip: nyasynth not installed");
            return;
        }
        // Discover UID via factory getClassInfo (any Audio Module Class).
        let binary = bundle_binary_path(&path);
        let library = unsafe { libloading::Library::new(&binary) }.unwrap();
        let get_factory: libloading::Symbol<'_, unsafe extern "C" fn() -> *mut IPluginFactory> =
            unsafe { library.get(b"GetPluginFactory\0") }.unwrap();
        let factory = unsafe { ComPtr::<IPluginFactory>::from_raw(get_factory()) }.unwrap();
        let count = unsafe { factory.countClasses() };
        let mut uid = None;
        for idx in 0..count {
            let mut info = vst3::Steinberg::PClassInfo {
                cid: [0; 16],
                cardinality: 0,
                category: [0; PClassInfo_::kCategorySize as usize],
                name: [0; PClassInfo_::kNameSize as usize],
            };
            if unsafe { factory.getClassInfo(idx, &mut info) } != kResultOk {
                continue;
            }
            if c_array_to_string(&info.category) != "Audio Module Class" {
                continue;
            }
            let mut s = String::new();
            for &b in &info.cid {
                use std::fmt::Write;
                let _ = write!(s, "{:02x}", b as u8);
            }
            uid = Some(s);
            break;
        }
        let uid = uid.expect("nyasynth should export an Audio Module Class");
        assert_eq!(bundle_is_instrument(&path, &uid), Some(true));
        assert!(name_looks_like_instrument("Nyasynth"));
    }
}
