//! Locate pre-bundled first-party VST3s and expose their paths to the DAW.
//!
//! Deliberately does **not** invoke `cargo` (nested cargo deadlocks on the
//! package lock). Bundle with the `cargo bundle-*` aliases or `build-daw`.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("cott-daw lives at crates/cott-daw");

    println!("cargo:rerun-if-changed=../cott-plugin-ui/src");
    println!("cargo:rerun-if-changed=../cott-xtask/src");

    for (stem, env_name, pretty, alias) in [
        ("cott-synth", "COTT_SYNTH_VST3", "CottSynth", "bundle-synth"),
        (
            "cott-filter",
            "COTT_FILTER_VST3",
            "CottFilter",
            "bundle-filter",
        ),
        (
            "cott-whistle",
            "COTT_WHISTLE_VST3",
            "CottWhistle",
            "bundle-whistle",
        ),
        ("cott-haze", "COTT_HAZE_VST3", "CottHaze", "bundle-haze"),
        ("cott-vinyl", "COTT_VINYL_VST3", "CottVinyl", "bundle-vinyl"),
        ("cott-tape", "COTT_TAPE_VST3", "CottTape", "bundle-tape"),
        ("cott-bass", "COTT_BASS_VST3", "CottBass", "bundle-bass"),
        ("cott-pluck", "COTT_PLUCK_VST3", "CottPluck", "bundle-pluck"),
        ("cott-kit", "COTT_KIT_VST3", "CottKit", "bundle-kit"),
    ] {
        println!("cargo:rerun-if-changed=../{stem}/src");
        println!("cargo:rerun-if-changed=../{stem}/Cargo.toml");
        println!("cargo:rerun-if-changed=../{stem}-dsp/src");
        let bundle = workspace.join(format!("target/bundled/{stem}.vst3"));
        println!("cargo:rerun-if-changed={}", bundle.display());
        if bundle.is_dir() {
            println!("cargo:rustc-env={env_name}={}", bundle.display());
        } else {
            println!(
                "cargo:warning={pretty}.vst3 not bundled yet — run `cargo {alias}` (or `cargo build-daw`)"
            );
        }
    }
}
