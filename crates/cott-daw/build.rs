//! Locate a pre-bundled CottSynth.vst3 and expose its path to the DAW.
//!
//! Deliberately does **not** invoke `cargo` (nested cargo deadlocks on the
//! package lock). Bundle with `cargo bundle-synth` or the `build-daw` alias.

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
    println!("cargo:rerun-if-changed=../cott-xtask/src");
    println!("cargo:rerun-if-changed={}", workspace.join("target/bundled/cott-synth.vst3").display());

    let bundled = workspace.join("target/bundled/cott-synth.vst3");
    if bundled.is_dir() {
        println!("cargo:rustc-env=COTT_SYNTH_VST3={}", bundled.display());
    } else {
        println!(
            "cargo:warning=CottSynth.vst3 not bundled yet — run `cargo bundle-synth` (or `cargo build-daw`)"
        );
    }
}
