//! Locate pre-bundled first-party VST3s and expose their paths to the DAW.
//!
//! Deliberately does **not** invoke `cargo` (nested cargo deadlocks on the
//! package lock). Bundle with `cargo bundle-synth` / `cargo bundle-filter`
//! or the `build-daw` script.

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("cott-daw lives at crates/cott-daw");

    println!("cargo:rerun-if-changed=../cott-synth/src");
    println!("cargo:rerun-if-changed=../cott-synth/Cargo.toml");
    println!("cargo:rerun-if-changed=../cott-synth-dsp/src");
    println!("cargo:rerun-if-changed=../cott-filter/src");
    println!("cargo:rerun-if-changed=../cott-filter/Cargo.toml");
    println!("cargo:rerun-if-changed=../cott-filter-dsp/src");
    println!("cargo:rerun-if-changed=../cott-xtask/src");

    let synth = workspace.join("target/bundled/cott-synth.vst3");
    println!("cargo:rerun-if-changed={}", synth.display());
    if synth.is_dir() {
        println!("cargo:rustc-env=COTT_SYNTH_VST3={}", synth.display());
    } else {
        println!(
            "cargo:warning=CottSynth.vst3 not bundled yet — run `cargo bundle-synth` (or `cargo build-daw`)"
        );
    }

    let filter = workspace.join("target/bundled/cott-filter.vst3");
    println!("cargo:rerun-if-changed={}", filter.display());
    if filter.is_dir() {
        println!("cargo:rustc-env=COTT_FILTER_VST3={}", filter.display());
    } else {
        println!(
            "cargo:warning=CottFilter.vst3 not bundled yet — run `cargo bundle-filter` (or `cargo build-daw`)"
        );
    }
}
