# Linkitylink - Development Documentation

## Overview

Linkitylink is a Planet Nine native "link-in-bio" app: a user builds one or more cards (name, bio, photo, category, up to 16 links), then publishes a card as a permanent, shareable SVG webpage — a Linktree/Beacons alternative. The primary product today is the Tauri iOS app in `app/`.

This directory also contains an older, separate Express server (`linkitylink.js` at the repo root) that predates the Tauri rewrite and now serves a different purpose — see "Root-level Express server" below. **Do not conflate the two** — they are unrelated codebases sharing one repo.

**Location (Tauri app)**: `/linkitylink/app/`
**Identifier**: `com.freyja.linkitylink`
**Stack**: Tauri 2 (Rust core in `src-tauri/`, vanilla JS/HTML/CSS frontend in `src/`), targeting iOS.
**Status**: Actively developed (commits through August 2026; TestFlight builds up to build 11 in `app/builds/v0.1.0/`).

## Architecture (Tauri app)

### Frontend (`app/src/`)

Three files, no framework or bundler:
- `index.html` — four views as sibling `<section>`s toggled via a `hidden` attribute: `cards-view` (grid of up to 4 cards), `edit-view`, `card-view`, `profile-view`.
- `main.js` (~975 lines) — all view logic, calling into the Rust backend via `core.invoke(...)`.
- `style.css` — dark/green/purple visual language shared across the Planet Nine app family.

### Backend (`app/src-tauri/src/lib.rs`, ~1275 lines)

| Command | Purpose |
|---|---|
| `get_categories` | Returns the shared business-category taxonomy (same slug/label list as idothis/bizbuz, copy-pasted, not a shared crate) |
| `load_cards` / `save_card` / `delete_card` | Local multi-card store (`cards.json` in the app data dir; up to `MAX_CARDS = 4`, `MAX_LINKS = 16` per card) |
| `publish_card` | Renders the card to SVG server-side (in Rust) and publishes it to BDO under a per-card sessionless keypair |
| `import_links` | Scrapes an external profile URL (Linktree gets a dedicated JSON-extraction parser; everything else falls back to a generic outbound-`<a>` scraper) and returns candidate links |
| `get_or_create_referral_link` | Publishes (once per install) a static "invite a friend" SVG to its own BDO record and returns the permanent share URL |
| `share_card_to_app_group` | Writes the chosen card's JSON into the shared iOS App Group under key `linkitylink.card`, so BizBuz can offer "Import from Linkitylink" |
| `get_shared_card_id` | Id of the card currently in the App Group (or `None`), so the UI can badge which one sibling apps read |
| `import_from_bizbuz` | Reads BizBuz's own shared profile (App Group key `bizbuz.profile`) and maps it into name/bio/photo/links |
| `load_canonical_profile` / `save_canonical_profile` | The cross-app Canonical Profile (see below) |

No server of its own — the app is a thin client over allyabase services, pointed at **`https://prod.8as.world/`** since September 2026 (it was on dev until the App Store submission round):
- **BDO** (`GATEWAY_BDO_URL`, `BDO_HASH = "linkitylink-card"`) — publishes each card as its own public record under its own per-card sessionless keypair (BDO's public storage is one slot per pubKey, so multi-card support requires one keypair per card, same pattern BizBuz uses).
- **savage** (`SAVAGE_URL`) — the service that actually serves a published BDO's embedded `svg` field as a live webpage at a pre-signed URL; `share_url` is computed locally (timestamp + signature) the instant `publish_card` returns, no server round trip needed to know the URL.

`GATEWAY_ENV = "prod-8as-world"` namespaces the BDO uuid per card (`bdo_uuid_by_env`) and per referral link, so pointing the app at a different gateway later won't collide with what's already published.

That namespacing matters more than it looks: **a uuid belongs to the base that minted it**, and a uuid from the wrong base fails silently — BDO answers `{"bdo": null}`, which parses as an empty record with no error at all. idothis had both of its shared features dead for weeks on exactly that, with nothing logged. Moving the gateway again means re-minting anything hardcoded and proving it with a functional round trip, not just a successful HTTP status.

### Card lifecycle

1. **Create/Edit** (`save_card`): local-only, written to `cards.json`. Links and photo never leave the device until publish.
2. **Import**: three ways to fill a card without typing every link by hand — `import_links` (scrape a Linktree/lnk.bio/etc. URL), `import_from_bizbuz` (pull from BizBuz's shared App Group profile, additive only — never overwrites a non-empty field), or the Canonical Profile's own "Fill from My Cards" pull (local-only, in `main.js`).
3. **Publish** (`publish_card`): renders the card as a self-contained SVG in Rust (`render_card_svg` — avatar circle w/ initials or photo, name, wrapped bio, one link row per entry with an auto-detected platform icon badge for ~30 recognized domains from Simple Icons/Font Awesome), embeds it as the `svg` field on the BDO record, and publishes/updates that record. Runs automatically in the background on every app launch (`backgroundPublishAllCards`) and after every save — there is no separate "did you remember to re-publish" step.

   Two details in the render worth keeping:
   - **The photo is base64'd into the SVG** as a `data:image/jpeg` `<image>`, since nothing here uploads files. savage's sanitizer used to strip every `data:` URI along with the scripts, so published cards rendered on the web with no photo while looking right in the app. savage now stashes base64 raster data URIs behind a placeholder, strips JavaScript, and restores them onto `<image>` elements only (png/jpeg/gif/webp, never `svg+xml`). If a published card loses its photo, that pass is the first place to look.
   - **Dark brand badges get a thin outline.** `badge_outline` adds a stroke when a platform colour's perceived luminance (`luminance`, YIQ weighting) is under 60, so a near-black brand mark doesn't disappear into the card's dark background.
4. **Share**: `share-sheet|share_text` (native iOS share sheet) with the pre-signed savage URL, or copy-to-clipboard from the card view.

   Since September 2026 sharing is also reachable straight from the card tile: each filled tile holds a full-bleed `.tile-open` button with a `.tile-share` quarter disc over its bottom-left corner, both labelled with an icon and a word. The tile body opens the card's detail view and Edit is one tap from there — tile → detail → edit, with share a step earlier than before. `border-radius` clips hit-testing, so the disc's curve really is its tap boundary (verified with `document.elementFromPoint`), and `.tile-share` needs the higher `z-index` or the open button underneath swallows the taps. BizBuz has the same pattern, sharing a vCard instead of a link.

### Two input fixes worth not regressing

- **The Save button used to lose edits.** Text typed into an add-row (a new link's label/URL, a new profile field) lived only in the DOM until a blur, so tapping Save straight after typing saved the card without it. Every input now writes through on `input`, submit runs `takeNewLink`/`takeNewProfileField` first, Enter `preventDefault`s into the same path, and opening the editor clears stale add-rows.
- **Link inputs must not auto-capitalise.** iOS capitalises the first letter of a text input by default, which turns `github.com` into `Github.com` and a brand label into a sentence. Every link label and URL input carries `autocapitalize="none"` plus `autocomplete="off"` and `spellcheck="false"`.

### Canonical Profile (shared across apps)

A third, independent record — separate from `cards`/`cards.json` entirely — synced via the **`group.club.home.front`** iOS App Group, matching the pattern used by BizBuz, getpayed and idothis. Read/written through `tauri_plugin_app_group`'s `read_value_sync`/`write_value_sync` under the key `canonical.profile`.

**Every app overwrites the whole record on save**, so `save_canonical_profile` has to carry forward every field this app has no UI for, or saving here erases what another app wrote:

| Field | Owned by | LinkityLink's job |
|---|---|---|
| `photo`, `fields` | shared — each app has an editor | edit |
| `address` | Gettit* | carry forward |
| `idothisCategories`, `serviceZip`, `idothisRateCents` | idothis | carry forward |
| `stripeConnected`, `payout` | getpayed | carry forward |

\*Gettit and Letemcook are named in these docs as family members but are
**not checked out at `~/Work`**, so nothing here was verified against them. Treat their ownership of `address` as inherited documentation, and assume the four apps in this workspace are the ones that actually have to agree.

`payout` is a `{pubKey, addieURL}` pair and travels as a unit — addie is distributed, so a pubkey alone only means something against the instance holding that account. LinkityLink reads it in `get_or_create_referral_link`, whose card asserts where money goes.

Adding a field to this record means adding it to **four** Rust structs, or the next app to save deletes it. The struct is intentionally copy-pasted rather than shared as a crate; `canonical_profile_round_trips_payout_key` in each app's `lib.rs` pins the contract by asserting an unknown future field survives a parse-and-save.

Linkitylink additionally participates in two narrower, app-to-app (not group-wide) handoffs via the same App Group plugin, both one-directional:
- `share_card_to_app_group` writes one user-chosen card to key `linkitylink.card` (BizBuz reads this for its own "Import from Linkitylink"). The App Group holds exactly one Linkitylink card, so each call replaces whatever was shared before. This is a deliberate button tap on the card view, **not** an automatic sync on save — an earlier version did sync silently on every save, which meant the last-saved card became the shared one with no way for the user to see or choose that. The cards grid badges the shared card ("Shared"), backed by `get_shared_card_id`, and `delete_card` clears the shared record if it deletes the card that was in it (the plugin has no delete, so an empty string is the tombstone).
- `import_from_bizbuz` reads BizBuz's own key, `bizbuz.profile`.

### Cross-promo

Saving a card whose category is food-related (`caterer`, `restauranteur`, `chef`, `food_cart`, `baker`) prompts (once ever, per card id, tracked in `localStorage`) to also list the business on **letemcook** via the `letemcook://add-location?name=...&bio=...` deep link.

### Unrelated to allyabase's Glyphenge proxy route

allyabase's own `CLAUDE.md` documents a wiki proxy alias `/plugin/allyabase/linkitylink/* → glyphenge:3010`. That is unrelated to this app — Glyphenge is allyabase's own server-side SVG-rendering microservice, and the alias exists only because it shares a name with the "linkitylink" product concept. Nothing in this Tauri app or the root Express server talks to that proxy route.

## Build & Deploy (Tauri app)

`app/package.json` scripts:
- `npm run dev` — `tauri dev`
- `npm run build` — desktop `tauri build`
- `npm run build:ios` — `scripts/build-ios.cjs`, the real distribution path (produces a signed IPA)
- `npm run ios:dev` / `npm run android:dev` / `npm run android:build` — also available, though iOS is the active target (`tauri.conf.json`'s window is a fixed phone size, and `build-ios.cjs` locks `TARGETED_DEVICE_FAMILY` to iPhone only)

### `scripts/build-ios.cjs`

1. Bumps `.build-number` (App Store Connect rejects re-uploading the same `CFBundleVersion`).
2. Wipes and regenerates `src-tauri/gen/apple/` via `tauri ios init` — a clean slate every time, so several things get patched back in immediately after since they don't survive regeneration:
   - `ios-native/` source path added to `project.yml` (carries `PrivacyInfo.xcprivacy`, the App Group UserDefaults usage declaration).
   - `TARGETED_DEVICE_FAMILY` restricted to iPhone only (the app's UI is a fixed phone-sized window, not designed for iPad, and building universal would also require iPad screenshots for App Store submission).
   - `ITSAppUsesNonExemptEncryption: false` — skips the encryption questionnaire on every upload (app only ever speaks HTTPS).
   - Real app icon (`src-tauri/icons/ios/*.png`) re-copied over `tauri ios init`'s stock icon, then every icon's alpha channel is flattened via `magick` (App Store rejects an alpha channel on the 1024×1024 marketing icon).
   - App Group entitlement (`group.club.home.front`) rewritten into `linkitylink_iOS.entitlements`, which `tauri ios init` otherwise regenerates as an empty `<dict/>`.
3. `tauri ios build --export-method app-store-connect --build-number <n>`, with a manual `xcodebuild -exportArchive` fallback for a known Xcode 26 quirk where Tauri's export sometimes fails even though the archive built fine. Before attempting the manual fallback, the script checks the keychain for an actual Apple Distribution signing identity and bails loudly if only a Development identity is present (rather than silently producing a wrongly-signed IPA that fails at upload with a confusing error).
4. Copies the resulting IPA to `app/builds/v{version}/{ProductName}-{buildNumber}.ipa`.

Upload to App Store Connect is a deliberately separate, manual step (Transporter.app or `xcrun altool`) — the script never uploads anything itself.

**`FORCE_COLOR` gotcha** (shared with all three sibling apps, now handled in the script): `xcodegen` resolves `${FORCE_COLOR}` in `project.yml`'s Run Script phase eagerly, from the generating shell, rather than deferring it to Xcode's build environment like the other `${VAR}` tokens beside it. Set — some terminals set it to `3` — that value lands in the argument list ahead of `${ARCHS}` and the build fails with `"3" isn't a known arch`, which reads like a toolchain problem and isn't. `build-ios.cjs` now does `delete process.env.FORCE_COLOR` itself.

**Version trains close.** Once a build is submitted under a version, App Store Connect rejects further uploads to it with `Validation failed (409) Invalid Pre-Release Train`. A new round needs a new version in `tauri.conf.json`, `package.json` and `Cargo.toml`, plus `.build-number` back to 0. Store version strings and the version here do not correspond and are not worth reconciling.

**Screenshots**: `app-store-screenshots/README.md` documents the process; `make-screenshots.sh` produces 6.9″ (1320×2868) and 6.5″ (1284×2778) sets from originals, and `SKIP_TF_MASK=1` skips the TestFlight banner mask that otherwise blacks out the status bar clock. A listing showing no screenshots in App Store *search results* means the 6.5″ set is missing.

### Native plugins

Two local Tauri plugins live under `app/src-tauri/`, both thin Swift/Rust wrappers with no business logic:
- **`tauri-plugin-app-group`** — `read_value_sync`/`write_value_sync` against the shared iOS App Group `group.club.home.front` (UserDefaults-backed). Used for both the cross-app Canonical Profile and the narrower BizBuz handoff.
- **`tauri-plugin-share-sheet`** — wraps `UIActivityViewController` so `share_text` can invoke the native iOS share sheet from JS (`plugin:share-sheet|share_text`). Linkitylink is the only sibling app observed with this plugin alongside `tauri-plugin-app-group`, since sharing a public link is core to what this app does (BizBuz/Gelder/etc. don't need a system share sheet for their primary flows the same way).

`bdo-rs` (the Rust BDO client) is pulled by relative path from `allyabase/deployment/bdo/src/client/rust/bdo-rs` — this app's Cargo.toml assumes the sibling `allyabase` checkout exists at `../../../allyabase` relative to `app/src-tauri/`. On this Mac those `allyabase/deployment/{bdo,addie}` directories are empty and have to be **symlinked** to the standalone `~/Work/bdo` and `~/Work/addie` checkouts, or the build can't resolve the path dependency. idothis takes the other approach and points straight at `../../../bdo/...` and `../../../prof/...`; both work, and the two styles coexist.

## Root-level Express server (`linkitylink.js`)

A separate, older Node/Express service at the repo root (`linkitylink.js`, `package.json` with `"main": "linkitylink.js"`, `lib/app-handoff.js`, `lib/relevant-bdos-middleware.js`, `public/*.html`, `docker-compose*.yml`, `Dockerfile*`) — **not part of the Tauri app**, and not wired into it in any way. It predates the Tauri rewrite (its own `README.md` still describes it as the current product, with API docs for `POST /create`, emojicode-based viewing, etc., matching what the old version of this file also described) but is **not dead code**: it's still current as of an April 2026 commit (`d9b2d71`, vs. August 2026 for the Tauri app), and it is what the Federated Wiki plugin `wiki-plugin-linkitylink` (in `/Users/zachbabb/Work/planet-nine/third-party/wiki-plugin-linkitylink/`) spawns as a child process under the Service-Bundling Plugin Pattern, proxying `/plugin/linkitylink/*` to it.

This server:
- Runs on port 6010 by default (`PORT` env var), talks to Fount/BDO/Addie over HTTP (`FOUNT_BASE_URL`/`BDO_BASE_URL`/`ADDIE_BASE_URL`), and creates link pages as public BDOs keyed by emojicode rather than the Tauri app's per-card keypair model.
- `lib/relevant-bdos-middleware.js` — Express middleware for the `relevantBDOs`/payee-splitting pattern used by the user-submitted-template creator-economy feature (template creators earn a cut via a payee-quad BDO referenced from the template).
- `lib/app-handoff.js` — a web-to-app handoff flow (authenticate a pending web-created BDO to a native app via a color-sequence game, then hand off the coordinating pubKey) — this is a different, older mechanism than the App Group-based Canonical Profile used by the Tauri app.
- `docker-compose.yml` builds and runs this server as a container alongside a `planetnine/allyabase` image; `docker-compose.standalone.yml`/`Dockerfile.local` are lighter-weight local variants.

For API/integration details of this server (endpoints, environment variables, the template-marketplace / MAGIC-spell revenue-sharing feature), see the wiki plugin's own documentation: `/Users/zachbabb/Work/planet-nine/third-party/wiki-plugin-linkitylink/CLAUDE.md`, and this repo's own `README.md` and `MANUAL-TESTING.md`/`USER-TESTING-GUIDE.md`.

## Known Limitations

- **Referral cards are published once and cached, so what they assert has to be correctable**: the record lives at a permanent savage URL that may already be shared widely, so `get_or_create_referral_link` updates it IN PLACE rather than minting a new link whenever something it asserts has changed — the App Store URL, or the payout destination getpayed writes to the shared profile after connecting Stripe. `ReferralLink` remembers both so a change can be detected; `app_store_url: None` means the record predates that tracking (i.e. carries the old placeholder) and is rewritten once. Linkitylink's listing id is **6808065892** (set September 2026); the public page 404s until the app is actually released.
- **Import scraping is best-effort**: `import_links`' generic fallback parser (non-Linktree URLs) is a simple outbound-`<a>` scrape with a same-origin/self-referential-share-widget filter; sites that render their link list via client-side JS won't produce any results (server-side `reqwest` fetch only, no headless browser).
- **Root Express server and Tauri app are fully independent products** sharing a repo and a name — there is no shared data model, shared code, or migration path between a card in the Tauri app and a link page created via the Express server's BDO/emojicode flow.

## Not done here yet

- **The Canonical Profile has no server-side copy.** It lives only in the App
  Group, and every sibling app overwrites the whole record on save. **prof**
  now exists for exactly that (PII encrypted at rest, owner-only, no route
  that hands one person's profile to another) and idothis ships the pattern:
  an opt-in "Profile Backup" card, `canonical_to_profile_data` /
  `profile_to_canonical`, and delete-before-key-deletion on reset. Copy
  idothis's rather than reinventing it. prof is not deployed yet, and the UI
  there hides itself until `/health` reports an encryption key.
- **App Store screenshots need recapturing from build 24 or later.** Two of
  the captured screens still show copy with em dashes in it, which were
  cleared from the app in September 2026 (house style: no em dashes in
  user-facing copy).
- **Store metadata and keywords** were never written up anywhere.

## Related Documentation

- **How these services fit together** — the seams, the silent failure modes,
  and where PII may and may not go: `allyabase/CLAUDE.md` → "How the apps and
  services fit together". Read that before changing anything that crosses a
  service.
- Federated Wiki plugin that bundles the root Express server: `/Users/zachbabb/Work/planet-nine/third-party/wiki-plugin-linkitylink/CLAUDE.md`
- Sibling apps sharing the same Tauri scaffolding and App Group: BizBuz, Gelder, Gettit, Letemcook, idothis (see their own `CLAUDE.md`)
- allyabase's own service list/proxy routing (for context on the *unrelated* Glyphenge alias): `/allyabase/CLAUDE.md`

## Last Updated
September 24, 2026 — Repointed at prod for App Store submission; documented
the render fixes (base64 photo into the SVG and savage's matching sanitizer
change, outlines on dark brand badges), the input fixes (write-through save,
no auto-capitalisation), the card-tile share/edit affordance, the Canonical
Profile field-ownership contract, and the `FORCE_COLOR` / version-train /
screenshot facts. App Store id **6808065892**.

September 2, 2026 — Full rewrite. The previous version of this file documented only the root-level Express server (`linkitylink.js`) as if it were the entire project — accurate for that server in isolation (its README/API haven't materially changed), but it predated and never mentioned the Tauri iOS app in `app/`, which is now the actively developed product. This version documents both, and clarifies they are separate, unconnected codebases.
