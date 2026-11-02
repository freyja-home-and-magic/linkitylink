#!/usr/bin/env node
// iOS build script — produces a signed IPA for TestFlight/App Store Connect.
//
//   1. Bump the build number (persisted in .build-number — App Store Connect
//      rejects re-uploading the same CFBundleVersion for an existing version)
//   2. Reinitialise the Tauri iOS project (clean slate — picks up Rust/config changes)
//   3. Patch the generated project — none of this is picked up by plain
//      `tauri ios init`/`tauri ios dev`, only by this script:
//        - TARGETED_DEVICE_FAMILY restricted to iPhone only
//        - ITSAppUsesNonExemptEncryption declared (skips the encryption
//          questionnaire on every App Store Connect upload)
//        - real app icon (icons/ios/*.png) re-copied over the default one
//          `tauri ios init` regenerates every time
//   4. tauri ios build --export-method app-store-connect --build-number <n>
//      (Xcode 26 sometimes rejects the "method" key in exportOptionsPlist with
//       EXPORT FAILED even though the xcarchive built fine; if that happens,
//       fall back to a manual xcodebuild -exportArchive call.)
//   5. Copy the IPA to builds/vX.X.X/
//
// Usage:
//   node scripts/build-ios.cjs
//   npm run build:ios

'use strict';

const fs = require('fs');
const path = require('path');
const { execSync, spawnSync } = require('child_process');

const ROOT = path.join(__dirname, '..');
const TEAM_ID = 'PCUFMY9NKZ';

// Xcode's "Build Rust Code" phase runs `tauri ios xcode-script ... ${FORCE_COLOR} ${ARCHS}`.
// If FORCE_COLOR is inherited as a bare value (e.g. "3") it lands in the arch list and
// tauri fails with "{arch} isn't a known arch". Tauri sets it itself when it wants it.
delete process.env.FORCE_COLOR;

// ── 1. Bump build number ────────────────────────────────────────────────────
//
// Everything exported for App Store Connect is a PROD build. Dev testing runs
// against the local Tauri dev server instead (npm run dev:ios), not through
// TestFlight. An odd-dev/even-prod scheme was tried on 2026-09-28 and taken
// back the same week: both halves share one bundle id and therefore one app
// container, so a dev build's local state (cards.json, base.json,
// bdo_keys.json, the addie identity) survives into the next prod install of
// the same app and quietly points it at the wrong base.
const buildNumberFile = path.join(ROOT, '.build-number');
const prevBuildNumber = fs.existsSync(buildNumberFile)
  ? parseInt(fs.readFileSync(buildNumberFile, 'utf8').trim(), 10) || 0
  : 0;
const buildNumber = prevBuildNumber + 1;
fs.writeFileSync(buildNumberFile, `${buildNumber}\n`);

const buildEnvName = 'prod';
console.log(`\n==> Build number: ${buildNumber} (${buildEnvName})`);

// The env cannot reach the compiler as a variable: the Rust crate is built
// inside an Xcode Run Script phase, which does not inherit one. idothis lost
// three consecutive IPAs to exactly that — they reported dev and shipped
// prod. So translate here, in the one process where the choice is visible,
// into src-tauri/.build-env, which build.rs reads and cargo tracks.
const setEnv = spawnSync(
  process.execPath, [path.join(__dirname, 'set-build-env.cjs'), buildEnvName],
  { stdio: 'inherit' }
);
if (setEnv.status !== 0) {
  console.error(`\n❌ '${buildEnvName}' is not a base this app can build against.`);
  process.exit(1);
}

// ── 2. Reinitialise Tauri iOS project ─────────────────────────────────────────
console.log('\n==> Reinitialising Tauri iOS project...');
const genApple = path.join(ROOT, 'src-tauri', 'gen', 'apple');
if (fs.existsSync(genApple)) {
  fs.rmSync(genApple, { recursive: true, force: true });
}
execSync('npx tauri ios init', { stdio: 'inherit', cwd: ROOT });

// ── 3. Patch generated iOS project ────────────────────────────────────────────
console.log('\n==> Patching iOS project...');

const projectYmlPath = path.join(genApple, 'project.yml');
let projectYml = fs.readFileSync(projectYmlPath, 'utf8');

// ── 3a0. ios-native/ — carries PrivacyInfo.xcprivacy (App Group UserDefaults
// usage declaration) into the app target's sources.
if (!projectYml.includes('../../ios-native')) {
  projectYml = projectYml.replace(
    '      - path: LaunchScreen.storyboard\n',
    '      - path: LaunchScreen.storyboard\n      - path: ../../ios-native\n'
  );
  console.log('    Added ios-native source path (carries PrivacyInfo.xcprivacy)');
}

// ── 3a. Restrict to iPhone — the app's UI is a fixed phone-sized window
// (see tauri.conf.json), so building universal (the XcodeGen default) would
// just stretch that layout across an iPad without ever having been designed
// for it, and would additionally require iPad screenshots for App Store
// Connect submission for a form factor the app doesn't actually support.
if (!projectYml.includes('TARGETED_DEVICE_FAMILY')) {
  projectYml = projectYml.replace(
    '      base:\n        ENABLE_BITCODE: false\n',
    '      base:\n        ENABLE_BITCODE: false\n        TARGETED_DEVICE_FAMILY: "1"\n'
  );
  console.log('    Restricted TARGETED_DEVICE_FAMILY to iPhone only');
}
if (projectYml.includes('UISupportedInterfaceOrientations~ipad')) {
  projectYml = projectYml.replace(
    /\n {8}UISupportedInterfaceOrientations~ipad:\n(?: {10}- UIInterfaceOrientation\w+\n)+/,
    '\n'
  );
  console.log('    Removed iPad-only orientation keys (iPhone-only target)');
}

// ── 3b. Declare exempt encryption — the app only ever speaks HTTPS, which
// is exempt from export compliance, but without this key App Store Connect
// re-asks the encryption questionnaire on every single upload.
if (!projectYml.includes('ITSAppUsesNonExemptEncryption')) {
  projectYml = projectYml.replace(
    '        UILaunchStoryboardName: LaunchScreen\n',
    '        UILaunchStoryboardName: LaunchScreen\n        ITSAppUsesNonExemptEncryption: false\n'
  );
  console.log('    Declared exempt encryption (HTTPS only, no export compliance prompt)');
}

fs.writeFileSync(projectYmlPath, projectYml);

console.log('    Regenerating .xcodeproj from patched project.yml...');
execSync('xcodegen generate --spec project.yml', { stdio: 'inherit', cwd: genApple });

// ── 3c. Re-copy the real app icon over `tauri ios init`'s default one ────────
// `tauri ios init` (step 2, above) wipes gen/apple/ and regenerates
// Assets.xcassets/AppIcon.appiconset/ from scratch every build - and what it
// puts there is NOT our actual icon (`tauri icon` only writes into an
// existing gen/apple/ as a side-effect convenience; it doesn't survive a
// fresh init). Without this step every real build silently reverts to
// Tauri's stock icon, which is why this matters even though it looks like a
// one-time setup concern. Mirrors the identical fix in bizbuz's build-ios.cjs.
const iconSrc = path.join(ROOT, 'src-tauri', 'icons', 'ios');
const iconDest = path.join(genApple, 'Assets.xcassets', 'AppIcon.appiconset');
const iconFiles = fs.readdirSync(iconSrc).filter((f) => f.endsWith('.png'));
iconFiles.forEach((f) => fs.copyFileSync(path.join(iconSrc, f), path.join(iconDest, f)));
console.log(`    Restored ${iconFiles.length} real app icons -> gen/apple/Assets.xcassets/AppIcon.appiconset/`);

// `tauri icon` always writes every size as RGBA, even from a fully opaque
// source — App Store Connect rejects an alpha channel on the 1024x1024
// marketing icon ("Invalid large app icon... can't be transparent or
// contain an alpha channel"). Flattening every copied icon here (not just
// the 1024 one) keeps the whole set consistent and survives a future
// `tauri icon` regeneration, since this runs on every build, not just once.
iconFiles.forEach((f) => {
  const p = path.join(iconDest, f);
  spawnSync('magick', [p, '-background', '#1F2933', '-alpha', 'remove', '-alpha', 'off', p]);
});
console.log('    Flattened alpha channel out of all app icons (App Store rejects it on the 1024x1024 marketing icon)');

// ── 3d. Patch App Group entitlements — `tauri ios init` (step 2) regenerates
// this as an empty <dict/> every build, same problem as the app icon above.
// project.yml already points CODE_SIGN_ENTITLEMENTS at this file
// automatically (XcodeGen's `entitlements.path` key), so only the file
// content itself needs patching, not project.yml.
const entitlementsPath = path.join(genApple, 'linkitylink_iOS', 'linkitylink_iOS.entitlements');
fs.writeFileSync(entitlementsPath, `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.application-groups</key>
    <array>
        <string>group.club.home.front</string>
    </array>
    <!-- The HomeFront identity every app shares (tauri-plugin-identity-keychain). -->
    <key>keychain-access-groups</key>
    <array>
        <string>$(AppIdentifierPrefix)club.home.front</string>
    </array>
</dict>
</plist>
`);
console.log('    Patched App Group and keychain group entitlements (club.home.front)');

// ── 4. Build IPA ───────────────────────────────────────────────────────────────
console.log('\n==> Building Linkitylink IPA...');
const conf = JSON.parse(fs.readFileSync(path.join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const ipaName = `${conf.productName}.ipa`;
const buildArm64 = path.join(genApple, 'build', 'arm64');
const ipaSrc = path.join(buildArm64, ipaName);

// Remove stale IPA so we can reliably detect a new successful build.
if (fs.existsSync(ipaSrc)) fs.rmSync(ipaSrc);

spawnSync(
  'npx', ['tauri', 'ios', 'build', '--export-method', 'app-store-connect', '--build-number', String(buildNumber)],
  { stdio: 'inherit', cwd: ROOT, shell: true }
);

// ── 4a. Xcode 26 fallback: manual xcodebuild export ──────────────────────────
if (!fs.existsSync(ipaSrc)) {
  console.log('\n==> Tauri export failed — checking why before attempting a manual export...');

  // The manual export below uses signingStyle: automatic with no explicit
  // export method, which silently re-signs with WHATEVER identity is in the
  // keychain rather than failing if that identity is the wrong type. If
  // there's no Apple Distribution identity at all, that produces an IPA that
  // "succeeds" locally but is actually signed for Development - Apple's
  // server-side validation then rejects it on upload with a confusing
  // "Invalid Provisioning Profile / Missing code-signing certificate" error
  // instead of failing here where the real cause is obvious. Bail out loudly
  // instead of taking that path.
  const identities = spawnSync('security', ['find-identity', '-v', '-p', 'codesigning'], {
    encoding: 'utf8',
  }).stdout || '';
  if (!/Apple Distribution|iOS Distribution/.test(identities)) {
    console.error('\n❌ No "Apple Distribution" (or legacy "iOS Distribution") signing certificate found in the keychain.');
    console.error('   Only a Development identity is available, which cannot produce a valid App Store build:');
    console.error(identities.split('\n').filter((l) => l.trim()).map((l) => `     ${l}`).join('\n'));
    console.error('\n   Fix: Xcode -> Settings -> Accounts -> select the team -> Manage Certificates -> "+" -> Apple Distribution.');
    console.error(`   (Team ${TEAM_ID} - must be an account with Admin/App Manager access to that team.)`);
    process.exit(1);
  }

  console.log('    Distribution identity present - export failure is likely the known Xcode 26 quirk, retrying manually...');

  const buildDir2 = path.join(genApple, 'build');
  if (!fs.existsSync(buildDir2)) {
    console.error('❌ Build directory not found — the compilation itself failed.');
    process.exit(1);
  }
  const archives = fs.readdirSync(buildDir2).filter((f) => f.endsWith('.xcarchive'));
  if (archives.length === 0) {
    console.error('❌ No xcarchive found — the build itself failed.');
    process.exit(1);
  }
  const archiveName = archives
    .map((f) => ({ f, mtime: fs.statSync(path.join(buildDir2, f)).mtime }))
    .sort((a, b) => b.mtime - a.mtime)[0].f;
  const xcarchive = path.join(buildDir2, archiveName);
  console.log(`    Archive: ${archiveName}`);

  const exportPlist = path.join(require('os').tmpdir(), 'linkitylink-export-options.plist');
  fs.writeFileSync(exportPlist, `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>destination</key>
    <string>export</string>
    <key>signingStyle</key>
    <string>automatic</string>
    <key>teamID</key>
    <string>${TEAM_ID}</string>
</dict>
</plist>
`);

  fs.mkdirSync(buildArm64, { recursive: true });

  const exportResult = spawnSync(
    'xcodebuild', [
      '-exportArchive',
      '-archivePath', xcarchive,
      '-exportOptionsPlist', exportPlist,
      '-exportPath', buildArm64,
      '-allowProvisioningUpdates',
    ],
    { stdio: 'inherit', cwd: ROOT }
  );

  if (exportResult.status !== 0 || !fs.existsSync(ipaSrc)) {
    console.error(`\n❌ Manual export also failed. IPA not found at ${ipaSrc}`);
    process.exit(1);
  }
  console.log('    Manual export succeeded.');
}

// ── 5. Copy IPA to builds/vX.X.X/ ────────────────────────────────────────────
const version = conf.version;
const buildDir = path.join(ROOT, 'builds', `v${version}`);
const versionedIpaName = `${conf.productName}-${buildNumber}.ipa`;

fs.mkdirSync(buildDir, { recursive: true });
fs.copyFileSync(ipaSrc, path.join(buildDir, versionedIpaName));

// ── Prove the built binary points where the build number says ────────────────
//
// The env reaches the compiler through a FILE (src-tauri/.build-env), because
// an iOS build compiles the Rust crate inside an Xcode Run Script phase that
// inherits no environment. idothis shipped three consecutive IPAs pointed at
// prod while reporting dev before that was understood. So the env var is not
// trusted here: the evidence is read back out of the IPA that was actually
// produced, and a mismatch fails the build. A test IPA on the wrong base is
// worse than no IPA, because it looks like it works.
const host = buildEnvName === 'prod' ? 'prod.8as.world' : 'dev.8as.world';
const otherHost = buildEnvName === 'prod' ? 'dev.8as.world' : 'prod.8as.world';
const expectations = [
  `https://${host}/bdo/`,
  `https://${host}/savage/`,
];
const forbidden = [
  `https://${otherHost}/bdo/`,
  `https://${otherHost}/savage/`,
];

const verifyDir = fs.mkdtempSync(path.join(require('os').tmpdir(), 'linkitylink-verify-'));
spawnSync('unzip', ['-q', '-o', ipaSrc, '-d', verifyDir]);
const verifyBinary = path.join(
  verifyDir, 'Payload', `${conf.productName}.app`, conf.productName
);
if (!fs.existsSync(verifyBinary)) {
  console.error(`\n❌ Couldn't read ${conf.productName} out of the IPA to verify its base.`);
  console.error(`   Looked for ${verifyBinary}`);
  process.exit(1);
}
const symbols = spawnSync('strings', ['-a', verifyBinary], {
  encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
}).stdout || '';
fs.rmSync(verifyDir, { recursive: true, force: true });

const missing = expectations.filter((v) => !symbols.includes(v));
const strays = forbidden.filter((v) => symbols.includes(v));
if (missing.length || strays.length) {
  console.error(`\n❌ The built IPA does not point at ${buildEnvName}.`);
  for (const m of missing) console.error(`   Expected ${m}, which is not in the binary.`);
  for (const x of strays) console.error(`   Found another base's value: ${x}`);
  console.error(`   Check src-tauri/.build-env (should say '${buildEnvName}'), then:`);
  console.error(`     touch src-tauri/build.rs && npm run build:ios`);
  console.error(`\n   builds/v${version}/${versionedIpaName} is NOT trustworthy. Do not upload it.\n`);
  process.exit(1);
}
console.log(`\n🔎 Verified in the IPA: this is a ${buildEnvName} build (build ${buildNumber})`);

console.log(`\n✅ Build complete: builds/v${version}/${versionedIpaName} (build ${buildNumber})`);
console.log('   Upload via Transporter.app, or:');
console.log(`   xcrun altool --upload-app -f builds/v${version}/${versionedIpaName} -t ios --apiKey <KEY_ID> --apiIssuer <ISSUER_ID>\n`);
