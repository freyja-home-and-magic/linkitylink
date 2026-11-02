#!/usr/bin/env node
//
// Writes which allyabase base this build talks to into src-tauri/.build-env,
// which lib.rs reads with include_str! at compile time.
//
// Why a file and not an environment variable: an iOS build compiles the Rust
// crate inside an Xcode Run Script phase, and LINKITYLINK_ENV does NOT reach it.
// `LINKITYLINK_ENV=dev npm run build:ios` produced three consecutive IPAs (20, 21,
// 22) pointed at prod while reporting success — build.rs confirmed it, logging
// `rerun-if-env-changed=LINKITYLINK_ENV` and no value beside it. A file is part of
// the source tree, so rustc records it as a dependency and cargo recompiles
// when it changes, on every build path.
//
// Usage: node scripts/set-build-env.cjs <prod|dev|test|local>

const fs = require('fs');
const path = require('path');

const SRC_TAURI = path.join(__dirname, '..', 'src-tauri');
const TARGET = path.join(SRC_TAURI, '.build-env');
const BASES = path.join(SRC_TAURI, 'bases.json');

const bases = JSON.parse(fs.readFileSync(BASES, 'utf8'));
const ENVS = Object.keys(bases).filter((k) => !k.startsWith('$'));

const requested = (process.argv[2] || '').trim();

if (!ENVS.includes(requested)) {
  console.error(`Usage: node scripts/set-build-env.cjs <${ENVS.join('|')}>`);
  console.error(`Got: ${requested || '(nothing)'}`);
  process.exit(1);
}

// One level of aliasing, matching build.rs: `test` is the dev base under
// another name and shares its records.
const resolved = bases[requested].aliasOf || requested;
const cfg = bases[resolved];

const current = fs.existsSync(TARGET) ? fs.readFileSync(TARGET, 'utf8').trim() : null;
fs.writeFileSync(TARGET, `${requested}\n`);

if (current === requested) {
  console.log(`Build env: ${requested}`);
} else {
  console.log(`Build env: ${current || '(unset)'} -> ${requested}`);
}
