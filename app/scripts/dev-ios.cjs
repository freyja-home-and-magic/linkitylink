#!/usr/bin/env node
//
// Runs this app against the LOCAL Tauri dev server, pointed at dev.8as.world.
//
//   npm run dev:ios        # on a device or simulator
//
// Why this exists rather than a dev TestFlight build: a dev build and a prod
// build share one bundle id, and therefore one app container. Installing one
// over the other leaves the first one's local state behind — cards.json,
// base.json, bdo_keys.json, the addie identity — and that state names a base.
// The result is a prod app quietly reading dev's records, which fails silently
// because BDO answers a foreign uuid with {"bdo": null} rather than an error.
// Keeping dev off TestFlight entirely is the only version of this that can't
// go wrong. Everything exported for App Store Connect is prod.
//
// The one thing `tauri ios dev` does NOT do for us is the App Group
// entitlement. scripts/build-ios.cjs writes it on every release build because
// `tauri ios init` regenerates the file empty; without it the Canonical
// Profile, "Import from ..." between the sibling apps, and GetPayed's
// stripeConnected handoff to idothis are all dead. So this patches the same
// file before handing over to the dev server.
//
// Note `tauri ios dev` only runs `tauri ios init` when gen/apple is missing,
// so the patch survives across runs. If you ever delete gen/apple by hand,
// this script puts it back correctly on the next run.

const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const XCODE_TARGET = 'linkitylink_iOS';
const APP_GROUP = 'group.club.home.front';

// 1. Point the Rust crate at dev. set-build-env.cjs refuses an env that is
//    not in bases.json, and (for idothis) one whose records are unminted.
const setEnv = spawnSync(
  process.execPath, [path.join(__dirname, 'set-build-env.cjs'), 'dev'],
  { stdio: 'inherit' }
);
if (setEnv.status !== 0) process.exit(setEnv.status || 1);


// 2. Make sure the Xcode project exists before trying to patch it.
const genApple = path.join(ROOT, 'src-tauri', 'gen', 'apple');
if (!fs.existsSync(genApple)) {
  console.log('\n==> gen/apple missing, running tauri ios init...');
  const init = spawnSync('npx', ['tauri', 'ios', 'init'], { stdio: 'inherit', cwd: ROOT });
  if (init.status !== 0) process.exit(init.status || 1);
}

// 3. The App Group entitlement, which tauri ios init leaves as an empty dict.
const entitlementsPath = path.join(genApple, XCODE_TARGET, `${XCODE_TARGET}.entitlements`);
if (!fs.existsSync(path.dirname(entitlementsPath))) {
  console.error(`\n❌ Expected ${path.dirname(entitlementsPath)} to exist after init.`);
  console.error('   The Xcode target name may have changed; check XCODE_TARGET in this script.');
  process.exit(1);
}
fs.writeFileSync(entitlementsPath, `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.application-groups</key>
    <array>
        <string>${APP_GROUP}</string>
    </array>
    <key>keychain-access-groups</key>
    <array>
        <string>$(AppIdentifierPrefix)club.home.front</string>
    </array>
</dict>
</plist>
`);
console.log(`\n==> dev build: base=dev.8as.world, App Group=${APP_GROUP}`);
console.log('    Everything for App Store Connect is prod; this never goes to TestFlight.\n');

// 4. Hand over.
const dev = spawnSync('npx', ['tauri', 'ios', 'dev'], { stdio: 'inherit', cwd: ROOT });
process.exit(dev.status || 0);
