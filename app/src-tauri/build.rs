use std::fs;
use std::path::Path;

use serde_json::Value;

/// Which base a build talks to is decided HERE, not in lib.rs, so the compiled
/// binary contains exactly one gateway: the one it will use.
///
/// The input is `src-tauri/.build-env` (written by scripts/set-build-env.cjs)
/// naming an entry in `src-tauri/bases.json`. `.build-env` is deliberately a
/// file rather than an environment variable — an iOS build compiles this crate
/// inside an Xcode Run Script phase, which does not inherit one. idothis lost
/// three consecutive IPAs to exactly that: they reported dev and shipped prod.
///
/// Choosing here also keeps the selection out of the optimizer's hands. A
/// runtime match would leave every candidate URL in the binary, and "the right
/// URL is present" would stop being evidence of anything — which is what
/// scripts/build-ios.cjs checks the finished IPA for.
fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets this");
    let build_env_file = Path::new(&manifest).join(".build-env");
    let bases_file = Path::new(&manifest).join("bases.json");

    // Cargo does not know these files matter unless it is told, and without
    // this nothing rebuilds when the target base changes.
    println!("cargo:rerun-if-changed={}", build_env_file.display());
    println!("cargo:rerun-if-changed={}", bases_file.display());

    let requested = fs::read_to_string(&build_env_file)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "prod".to_string());

    let raw = fs::read_to_string(&bases_file)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", bases_file.display()));
    let bases: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", bases_file.display()));

    // Anything unrecognised means prod. A mistyped env must not quietly point
    // a release at a developer's laptop.
    let mut name = match bases.get(&requested) {
        Some(_) if !requested.starts_with('$') => requested,
        _ => "prod".to_string(),
    };
    // One level of aliasing, so `test` is the dev base under another name.
    if let Some(alias) = bases
        .get(&name)
        .and_then(|c| c.get("aliasOf"))
        .and_then(Value::as_str)
    {
        name = alias.to_string();
    }
    let host = bases[&name]["host"]
        .as_str()
        .unwrap_or_else(|| panic!("bases.json '{name}' has no host"));

    println!("cargo:rustc-env=LINKITYLINK_BUILD_ENV={name}");
    println!("cargo:rustc-env=GATEWAY_BDO_URL=https://{host}/bdo/");
    println!("cargo:rustc-env=SAVAGE_URL=https://{host}/savage/");
    // eumachia hosts the tip pages a card's tip buttons link to (savage strips
    // scripts, so a card page can't ask for an amount itself).
    println!("cargo:rustc-env=EUMACHIA_URL=https://{host}/eumachia/");
    // Published uuids are stored per-env under this key, because a uuid minted
    // against one gateway 404s an update against another. Deriving it from the
    // host rather than hand-maintaining it means the two can never disagree.
    println!("cargo:rustc-env=GATEWAY_ENV={}", host.replace('.', "-"));


    // The build number, for `x-homefront-app-version`. It lives in
    // app/.build-number, which scripts/build-ios.cjs bumps immediately before
    // cargo runs, so the value compiled in is the one that ships as
    // CFBundleVersion. Missing (a bare `cargo test`, a fresh clone) is not an
    // error; the header just reads +0.
    let build_number_file = Path::new(&manifest).join("..").join(".build-number");
    println!("cargo:rerun-if-changed={}", build_number_file.display());
    let build_number = fs::read_to_string(&build_number_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0".to_string());
    println!("cargo:rustc-env=APP_BUILD_NUMBER={build_number}");

    tauri_build::build()
}
