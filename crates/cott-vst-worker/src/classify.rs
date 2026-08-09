//! Enrich VST3 scan metadata — truce-rack currently hardcodes
//! `category = Effect` and `accepts_midi = false` for every plugin.
//!
//! For yabridge bundles we also read Steinberg `moduleinfo.json` (when present)
//! so FX vs instruments are not guessed from the path alone.

use std::path::Path;

use tracing::debug;
use vst3::ComPtr;
use vst3::Steinberg::{
    IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo_,
    PClassInfo2, PClassInfo2_, TUID, kResultOk,
};

/// Resolved instrument/effect classification for catalog + load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassHint {
    pub is_instrument: bool,
    pub is_effect: bool,
}

impl ClassHint {
    pub fn instrument() -> Self {
        Self {
            is_instrument: true,
            is_effect: false,
        }
    }

    pub fn effect() -> Self {
        Self {
            is_instrument: false,
            is_effect: true,
        }
    }

    pub fn both() -> Self {
        Self {
            is_instrument: true,
            is_effect: true,
        }
    }
}

/// Best-effort classify a VST3 bundle without starting Wine when possible.
pub fn classify_bundle(bundle: &Path, prefer_instrument: bool) -> ClassHint {
    let name = bundle
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Plugin");

    if let Some(hint) = moduleinfo_class_hint(bundle) {
        // Bare ["Fx"] yields an empty hint — fall through to name heuristics.
        if hint.is_instrument || hint.is_effect {
            debug!(plugin = %name, ?hint, "classified from moduleinfo.json");
            return hint;
        }
    }

    if let Some(hint) = name_class_hint(name) {
        debug!(plugin = %name, ?hint, "classified from name heuristics");
        return hint;
    }

    // Unknown yabridge shells: offer both so the user can pick, but prefer the
    // instrument load path when requested (MIDI track attach).
    if prefer_instrument {
        ClassHint::both()
    } else {
        ClassHint::effect()
    }
}

/// Refine classification after the module is loaded (factory + name + moduleinfo).
pub fn classify_loaded(bundle: &Path, name: &str, uid_hex: &str, accepts_midi: bool) -> ClassHint {
    if let Some(hint) = moduleinfo_class_hint(bundle)
        && (hint.is_instrument || hint.is_effect)
    {
        return hint;
    }
    if let Some(is_inst) = bundle_is_instrument(bundle, uid_hex) {
        return if is_inst {
            ClassHint::instrument()
        } else {
            ClassHint::effect()
        };
    }
    if accepts_midi {
        return ClassHint::instrument();
    }
    if let Some(hint) = name_class_hint(name) {
        return hint;
    }
    // Last resort: MIDI-capable name tokens → instrument, else effect.
    if name_looks_like_instrument(name) {
        ClassHint::instrument()
    } else {
        ClassHint::effect()
    }
}

/// Read Steinberg `Contents/Resources/moduleinfo.json` Sub Categories.
fn moduleinfo_class_hint(bundle: &Path) -> Option<ClassHint> {
    let path = moduleinfo_path(bundle)?;
    let raw = std::fs::read_to_string(&path).ok()?;
    // Some vendor files ship with trailing commas / UTF-8 BOM.
    let cleaned = strip_trailing_commas(raw.trim_start_matches('\u{feff}'));
    let value: serde_json::Value = serde_json::from_str(&cleaned).ok()?;
    let classes = value.get("Classes")?.as_array()?;
    for class in classes {
        let category = class.get("Category")?.as_str().unwrap_or("");
        if category != "Audio Module Class" {
            continue;
        }
        let subs = class.get("Sub Categories")?;
        let parts: Vec<String> = match subs {
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_ascii_lowercase()))
                .collect(),
            serde_json::Value::String(s) => s
                .split('|')
                .map(|p| p.trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .collect(),
            _ => continue,
        };
        if parts.is_empty() {
            continue;
        }
        return Some(subcategories_to_hint(&parts));
    }
    None
}

fn moduleinfo_path(bundle: &Path) -> Option<std::path::PathBuf> {
    let direct = bundle.join("Contents").join("Resources").join("moduleinfo.json");
    if direct.is_file() {
        return Some(direct);
    }
    // yabridge: Resources may be a symlink into the Wine prefix bundle.
    let win_dir = bundle.join("Contents").join("x86_64-win");
    if win_dir.is_dir() {
        for entry in std::fs::read_dir(&win_dir).ok()?.flatten() {
            let p = entry.path();
            let target = p.canonicalize().unwrap_or(p);
            // Legacy single-file .vst3 — no Resources.
            // Bundle: .../Plugin.vst3/Contents/x86_64-win/Plugin.vst3 → sibling Resources
            if let Some(contents) = target.parent().and_then(|p| p.parent()) {
                let mi = contents.join("Resources").join("moduleinfo.json");
                if mi.is_file() {
                    return Some(mi);
                }
            }
        }
    }
    None
}

fn subcategories_to_hint(parts: &[String]) -> ClassHint {
    let has_instrument = parts.iter().any(|p| {
        matches!(
            p.as_str(),
            "instrument" | "synth" | "sampler" | "drum" | "piano"
        ) || p.contains("instrument")
    });
    if has_instrument {
        return ClassHint::instrument();
    }

    // Specific FX families → effect. Bare ["fx"] is ambiguous (some vendors tag
    // synths as Fx), so fall through to name heuristics.
    const FX_FAMILY: &[&str] = &[
        "eq",
        "dynamics",
        "reverb",
        "delay",
        "modulation",
        "distortion",
        "spatial",
        "pitch shift",
        "filter",
        "analyzer",
        "mastering",
        "restoration",
        "tools",
        "network",
        "surround",
    ];
    let has_fx_family = parts.iter().any(|p| FX_FAMILY.iter().any(|f| p == f));
    if has_fx_family {
        return ClassHint::effect();
    }

    // Only generic "fx" / "generator" etc. — unknown.
    // Return None by using a sentinel: caller handles via name. We encode as
    // both=false/false and let caller check — cleaner to return Option.
    // Here: treat as no moduleinfo signal.
    ClassHint {
        is_instrument: false,
        is_effect: false,
    }
}

/// Name-based classification when moduleinfo is missing or ambiguous (`Fx` only).
fn name_class_hint(name: &str) -> Option<ClassHint> {
    if name_looks_like_effect(name) {
        return Some(ClassHint::effect());
    }
    if name_looks_like_instrument(name) {
        return Some(ClassHint::instrument());
    }
    None
}

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
            let parts: Vec<String> = subs
                .split('|')
                .map(|p| p.trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .map(|s| s.to_string())
                .collect();
            let hint = subcategories_to_hint(&parts);
            // Ambiguous bare Fx → None so callers can use name heuristics.
            if !hint.is_instrument && !hint.is_effect {
                return None;
            }
            debug!(
                plugin = %c_array_to_string(&info.name),
                subcategories = %subs,
                is_instrument = hint.is_instrument,
                "classified VST3 factory"
            );
            return Some(hint.is_instrument);
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
    // Effects first — several Cymatics names look "instrument-y" but are FX.
    if name_looks_like_effect(name) {
        return false;
    }
    // "Surge XT Effects" / "Firefly Synth 2 FX" / "CardinalFX" are processors.
    if n.contains("effect")
        || n.ends_with(" fx")
        || n.contains(" fx ")
        || n.contains("cardinalfx")
        || (n.contains("surge") && n.contains("effect"))
    {
        return false;
    }
    // Explicit instrument product tokens (before generic needles).
    const PRODUCTS: &[&str] = &[
        "pandora", // sample instrument engine
        "lotus",
        "dreamscape",
        "dark sky",
        "neptune",
        "ocean pluck",
        "ocean",
        "quake",
        "ripple",
        "vortex",
        "voxity",
        "velvet",
        "halo",
        "origin",
        // common Linux-native instruments (hosts often mis-tag as Fx)
        "surge xt",
        "surge",
        "vitalium",
        "vital",
        "odin",
        "nyasynth",
        "nya",
        "squelch",
        "flechtwerk",
        "firefly synth",
        "geonkick",
        "cardinalsynth",
        "cardinal", // CardinalFX excluded above
        // generic
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
        "pluck",
        "vox",
    ];
    PRODUCTS.iter().any(|needle| n.contains(needle))
}

/// Conservative effect-name hints (used when skipping factory probes).
pub fn name_looks_like_effect(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    const PRODUCTS: &[&str] = &[
        // Cymatics processors (not instruments)
        "deja vu",
        "deja",
        "memory",   // analog chorus
        "pluto",    // multi FX
        "diablo",   // drum punch/clip FX (Lite + full)
        "cl-2a",
        "eqc-1a",
        "eqc",
        "lc-76",
        "mix link",
        "corrosion",
        "shifter",
        "omnivox",  // pitch / voice FX
        "illusion", // pitch shift
        // generic
        "eq",
        "comp",
        "reverb",
        "delay",
        "limiter",
        "gate",
        "filter",
        "utility",
        "analyzer",
        "saturat",
        "distort",
        "chorus",
        "flanger",
        "phaser",
        "compressor",
        "limiter",
    ];
    // "space" alone is too ambiguous; require reverb context or exact product.
    // Cymatics Space is a reverb; bare "space" elsewhere is too ambiguous.
    if n.contains("cymatics") && n.contains("space") {
        return true;
    }
    // Short tokens like "eq" must not match inside "effects" / "sequence".
    PRODUCTS.iter().any(|needle| {
        if needle.len() <= 3 {
            name_has_token(&n, needle)
        } else {
            n.contains(needle)
        }
    })
}

fn name_has_token(name: &str, token: &str) -> bool {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part == token)
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

fn strip_trailing_commas(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
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

    #[test]
    fn name_heuristics_cymatics() {
        assert!(name_looks_like_effect("Cymatics CL-2A"));
        assert!(name_looks_like_effect("Cymatics EQC-1A"));
        assert!(name_looks_like_effect("Cymatics Mix Link"));
        assert!(name_looks_like_effect("Cymatics Shifter"));
        assert!(name_looks_like_effect("Cymatics Deja Vu"));
        assert!(name_looks_like_effect("Cymatics Memory"));
        assert!(name_looks_like_effect("Cymatics Pluto"));
        assert!(name_looks_like_effect("Cymatics Diablo Lite"));
        assert!(!name_looks_like_instrument("Cymatics Diablo Lite"));
        assert!(name_looks_like_instrument("Cymatics Pandora"));
        assert!(name_looks_like_instrument("Cymatics Lotus"));
        assert!(!name_looks_like_effect("Cymatics Pandora"));
        assert!(name_looks_like_instrument("Surge XT"));
        assert!(!name_looks_like_instrument("Surge XT Effects"));
        assert!(name_looks_like_instrument("Vital"));
        assert!(name_looks_like_instrument("Odin2"));
        assert!(name_looks_like_instrument("SquelchBox"));
        assert!(name_looks_like_instrument("Flechtwerk"));
        assert!(name_looks_like_instrument("Firefly Synth 2"));
        assert!(!name_looks_like_instrument("Firefly Synth 2 FX"));
    }

    #[test]
    fn subcategories_instrument_and_fx() {
        assert_eq!(
            subcategories_to_hint(&["instrument".into(), "synth".into()]),
            ClassHint::instrument()
        );
        assert_eq!(
            subcategories_to_hint(&["fx".into(), "dynamics".into()]),
            ClassHint::effect()
        );
        // Bare Fx is ambiguous
        assert_eq!(
            subcategories_to_hint(&["fx".into()]),
            ClassHint {
                is_instrument: false,
                is_effect: false
            }
        );
    }
}
