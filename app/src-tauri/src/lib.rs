use bdo_rs::BDO;
use serde::{Deserialize, Serialize};
use sessionless::hex::IntoHex;
use sessionless::secp256k1::SecretKey;
use sessionless::Sessionless;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Manager;

// Path-based routing on the dev.8as.world droplet: nginx terminates TLS on
// 443 and proxies /<service>/ to that service's local port (bdo → 3003).
// One hostname, one certificate, everything over real HTTPS — which is also
// what keeps iOS ATS happy, since the services themselves speak plain HTTP
// and are not reachable directly from outside.
//
// Production, as of the App Store submission. Dev's BDO is a commit behind
// and answers a successful delete with 404, which made "Delete All My Data"
// report failure on a deletion that had in fact happened — not something to
// ship while claiming users can delete their data. Cards published against
// dev by earlier TestFlight builds are unaffected: their uuids are kept
// per-env (see GATEWAY_ENV), and unpublish_everywhere rebuilds each base
// from the stored env key, so deleting one still reaches the base holding
// it.
const GATEWAY_BDO_URL: &str = "https://prod.8as.world/bdo/";
const SAVAGE_URL: &str = "https://prod.8as.world/savage/";
const BDO_HASH: &str = "linkitylink-card";

// ── Palette ──────────────────────────────────────────────────────────────────
//
// HomeVentory dark, matching the app's own dark mode in style.css (the
// `prefers-color-scheme: dark` block: #1a1a1a ground, glacier-white text).
// Dark is the default look of the app, so the published card matches it.
//
// Evergreen stays for FILLED surfaces — the avatar circle and the button —
// but not for text on the ground: #2E5E4E on #1a1a1a is about 2.3:1, too low
// to read. Text that used to be evergreen (the name, the link arrows) uses
// soft mint instead, which is in the same HomeVentory palette.
//
// PALETTE_* is also sent to savage on publish (see publish_card), which themes
// the page chrome around the card. Without it savage falls back to BizBuz's
// old colours for every app it serves.
const PALETTE_BG: &str = "#1a1a1a";        // app dark-mode ground
const PALETTE_GREEN: &str = "#2E5E4E";     // deep evergreen
const PALETTE_GREEN_DARK: &str = "#1F4A3E";
const PALETTE_ACCENT: &str = "#4FA3F7";    // signal blue
const PALETTE_MINT: &str = "#AEE1D6";      // soft mint — accent text on dark
/// Text drawn ON an evergreen fill (avatar initials, button label). Was
/// written as {BG}, which only worked while BG happened to be light.
const PALETTE_ON_GREEN: &str = "#F7F9FA";  // glacier white
/// Glacier white (#F7F9FA) as rgb components — the ink on the dark ground. The published SVGs need the ink
/// colour at several opacities, and a Rust raw string can't carry an inline
/// hex literal (`r#"..."#` terminates at the first `"#`), so these are
/// interpolated as rgba(...) rather than written as hex.
const PALETTE_INK_RGB: &str = "247,249,250";
const MAX_CARDS: usize = 4;

// BDO mints its own server-side uuid on create_user, distinct from the local
// keypair — a uuid minted against one gateway 404s an update_bdo call on
// another, so both a card's publish record and the referral link are stored
// per-env (keyed by this const) rather than as a single value. Bump this
// whenever GATEWAY_BDO_URL points at a genuinely different BDO deployment.
const GATEWAY_ENV: &str = "prod-8as-world";
const MAX_LINKS: usize = 16;
// Bounds a pathological generic-fallback page's raw scrape output; the real
// user-facing cap is MAX_LINKS, enforced at merge time in the JS import handler.
const IMPORT_SCRAPE_CAP: usize = 40;

// Categories used to live here (per-app copy of idothis's taxonomy) and
// drove a Food & Drink cross-promo prompt for letemcook. Both have moved
// to the shared Canonical Profile — idothis is now the sole owner of the
// category taxonomy, and Linkitylink no longer has a `category` field on
// its cards.

// ── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LinkEntry {
    /// Client-generated (timestamp + counter), stable across reorders/edits —
    /// NOT derived from the url, which can repeat or change.
    pub id: String,
    pub label: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LinkCard {
    #[serde(default)]
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// Base64-encoded JPEG (no `data:` prefix), resized/cropped client-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photo: Option<String>,
    #[serde(default)]
    pub links: Vec<LinkEntry>,
    /// BDO identity uuid this card is published under, per environment
    /// (see `GATEWAY_ENV`) — a fresh env has no entry until first publish.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub bdo_uuid_by_env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

// ── Storage ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
struct CardsStore {
    cards: Vec<LinkCard>,
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn cards_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("cards.json"))
}

/// Pre-multi-card storage location — a single `LinkCard` written directly as
/// JSON (no wrapping `cards` array). Migrated into cards.json on first load
/// if a user already had one card from before this feature existed.
fn legacy_card_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("link_card.json"))
}

fn write_cards(app: &tauri::AppHandle, store: &CardsStore) -> Result<(), String> {
    let path = cards_path(app)?;
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn read_cards(app: &tauri::AppHandle) -> Result<CardsStore, String> {
    let path = cards_path(app)?;
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|e| e.to_string()),
        Err(_) => {
            // Nothing at cards.json yet — check for a pre-multi-card
            // link_card.json and migrate it in as this user's first card.
            match fs::read_to_string(legacy_card_path(app)?) {
                Ok(contents) => {
                    let mut card: LinkCard =
                        serde_json::from_str(&contents).map_err(|e| e.to_string())?;
                    if card.id.is_empty() {
                        card.id = new_id();
                    }
                    let store = CardsStore { cards: vec![card] };
                    write_cards(app, &store)?;
                    Ok(store)
                }
                Err(_) => Ok(CardsStore::default()),
            }
        }
    }
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Nanosecond timestamp + a monotonic in-process counter, so two link
/// entries created back-to-back never collide even if the clock's actual
/// resolution is coarser than a nanosecond.
fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", nanos, counter)
}

fn unix_now_ms_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

// ── BDO publishing ───────────────────────────────────────────────────────────
//
// BDO's public storage is keyed by pubKey — one identity, one publicly
// retrievable slot, overwritten on every public write regardless of the
// `hash` used. So each card that gets published needs its own sessionless
// keypair, not one shared device identity — same pattern bizbuz uses for
// its own multi-card support.

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BdoKeypair {
    private_key_hex: String,
    pub_key_hex: String,
}

fn bdo_keys_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("bdo_keys.json"))
}

fn read_bdo_keys(app: &tauri::AppHandle) -> HashMap<String, BdoKeypair> {
    let path = match bdo_keys_path(app) {
        Ok(p) => p,
        Err(_) => return HashMap::new(),
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_bdo_keys(app: &tauri::AppHandle, keys: &HashMap<String, BdoKeypair>) -> Result<(), String> {
    let path = bdo_keys_path(app)?;
    let json = serde_json::to_string_pretty(keys).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn sessionless_from_hex(priv_key_hex: &str) -> Result<Sessionless, String> {
    let bytes = hex::decode(priv_key_hex).map_err(|e| e.to_string())?;
    let secret_key = SecretKey::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(Sessionless::from_private_key(secret_key))
}

/// This key's BDO identity if it already has one, without creating it.
///
/// Deletion must never mint a keypair: a card that was never published has no
/// remote record, and creating a key to "delete" one would be pointless churn.
fn existing_bdo_sessionless(app: &tauri::AppHandle, key: &str) -> Option<Sessionless> {
    read_bdo_keys(app)
        .get(key)
        .and_then(|k| sessionless_from_hex(&k.private_key_hex).ok())
}

/// Unpublishes every remote record a key has, across every base it published
/// to, and reports which bases failed.
///
/// An empty return means the remote is clean and local state is safe to drop.
/// Callers must not delete locally while it is non-empty: `bdo_uuid_by_env`
/// and the keypair are the only route back to those records, so discarding
/// them leaves the user's name, photo and links published with no way for
/// anyone — including us — to take them down.
async fn unpublish_everywhere(
    app: &tauri::AppHandle,
    key: &str,
    hash: &str,
    uuids_by_env: &HashMap<String, String>,
) -> Vec<String> {
    if existing_bdo_sessionless(app, key).is_none() {
        return Vec::new();
    }

    let mut failed = Vec::new();
    for (env_key, uuid) in uuids_by_env {
        // Sessionless isn't Clone and BDO::new takes ownership, so rebuild it
        // per base rather than holding one across the loop.
        let Some(sessionless) = existing_bdo_sessionless(app, key) else {
            failed.push(env_key.clone());
            continue;
        };
        // env keys are hostnames with dots swapped for dashes, so this reaches
        // the base that actually holds the record.
        let host = env_key.replace('-', ".");
        let client = BDO::new(Some(format!("https://{host}/bdo/")), Some(sessionless));
        if client.delete_user(uuid, hash).await.is_err() {
            failed.push(env_key.clone());
        }
    }
    failed
}

/// Returns a card's BDO identity, generating and persisting a fresh keypair
/// the first time a given card is published.
fn load_or_create_bdo_sessionless(app: &tauri::AppHandle, key: &str) -> Result<Sessionless, String> {
    let mut keys = read_bdo_keys(app);
    if let Some(existing) = keys.get(key) {
        return sessionless_from_hex(&existing.private_key_hex);
    }

    let session = Sessionless::new();
    keys.insert(
        key.to_string(),
        BdoKeypair {
            private_key_hex: session.private_key().to_hex(),
            pub_key_hex: session.public_key().to_hex(),
        },
    );
    write_bdo_keys(app, &keys)?;
    Ok(session)
}

// ── SVG card rendering ───────────────────────────────────────────────────────
//
// savage only renders whatever `svg` property is already present on a
// published BDO's data, and its sanitizer strips <script>/javascript:/data:
// content — so every link row is a plain <a href="https://..."> element,
// same as bizbuz's contact rows.

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn get_initials(name: &str) -> String {
    let initials: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    if initials.is_empty() {
        "?".to_string()
    } else {
        initials
    }
}

/// Greedy word-wrap at `max_chars` per line, capped at `max_lines`. If the
/// text doesn't fit, the last line is trimmed and given a trailing ellipsis
/// rather than silently dropping the overflow.
fn wrap_text(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut i = 0;

    while i < words.len() && lines.len() < max_lines {
        let word = words[i];
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if candidate.chars().count() > max_chars && !current.is_empty() {
            lines.push(current.clone());
            current.clear();
        } else {
            current = candidate;
            i += 1;
        }
    }

    let truncated = i < words.len();
    if !current.is_empty() {
        lines.push(current);
    }

    if truncated {
        if let Some(last) = lines.last_mut() {
            while last.chars().count() + 1 > max_chars && !last.is_empty() {
                last.pop();
            }
            last.push('…');
        }
    }

    lines
}

/// Truncates a single-line label to `max_chars`, appending an ellipsis if
/// anything was cut — unlike `wrap_text`, this never produces multiple
/// lines, since every link row has a fixed height the layout depends on.
fn truncate_label(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let mut truncated: String = chars[..max_chars.saturating_sub(1)].iter().collect();
    truncated.push('…');
    truncated
}

/// Ensures a user-entered url has a scheme, same rule bizbuz applies to its
/// `website` field — but unlike a plain "doesn't already start with
/// http(s)://, so prepend it" check, this also recognizes a *malformed*
/// scheme (missing colon, single slash — e.g. "https//example.org", which
/// iOS's URL-field keyboard autofill has been observed to produce) and
/// replaces it with a clean one, rather than blindly prepending another
/// "https://" in front of it and producing "https://https//example.org".
fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    let lower = trimmed.to_lowercase();

    let scheme_prefix = if lower.starts_with("https") {
        Some((5, "https://"))
    } else if lower.starts_with("http") {
        Some((4, "http://"))
    } else {
        None
    };

    if let Some((len, canonical_scheme)) = scheme_prefix {
        let rest = &trimmed[len..];
        // Only treat "http"/"https" as an actual scheme if what follows
        // looks like scheme punctuation (some run of `:`/`/`) — otherwise
        // it's just a domain that happens to start with those letters
        // (e.g. "httpster.com"), not a malformed scheme, and should be left
        // alone below.
        if rest.starts_with(':') || rest.starts_with('/') {
            let stripped = rest.trim_start_matches([':', '/']);
            return format!("{canonical_scheme}{stripped}");
        }
    }

    format!("https://{trimmed}")
}

// ── Social platform icons ────────────────────────────────────────────────────
//
// Recognized purely by matching the link's own domain (no extra field on
// LinkEntry, no picker UI) - an existing saved link automatically picks up
// its brand icon the next time it's rendered if its host matches a known
// platform. Icon paths are from Simple Icons (MIT) via jsdirect CDN fetch;
// LinkedIn isn't in Simple Icons (dropped, presumably for trademark
// caution), so it comes from Font Awesome Free's brand set instead - hence
// the differing view_size (Simple Icons is a uniform 24x24 viewBox,
// LinkedIn's is 448x512).

struct PlatformIcon {
    name: &'static str,
    hex: &'static str,
    view_size: f64,
    domains: &'static [&'static str],
    path: &'static str,
}

const PLATFORMS: &[PlatformIcon] = &[
    PlatformIcon { name: "Amazon", hex: "#FF9900", view_size: 24.0, domains: &["amazon.com", "amazon.co.uk", "amazon.ca", "amazon.de"], path: "M.045 18.02c.072-.116.187-.124.348-.022 3.636 2.11 7.594 3.166 11.87 3.166 2.852 0 5.668-.533 8.447-1.595l.315-.14c.138-.06.234-.1.293-.13.226-.088.39-.046.525.13.12.174.09.336-.12.48-.256.19-.6.41-1.006.654-1.244.743-2.64 1.316-4.185 1.726a17.617 17.617 0 01-10.951-.577 17.88 17.88 0 01-5.43-3.35c-.1-.074-.151-.15-.151-.22 0-.047.021-.09.051-.13zm6.565-6.218c0-1.005.247-1.863.743-2.577.495-.71 1.17-1.25 2.04-1.615.796-.335 1.756-.575 2.912-.72.39-.046 1.033-.103 1.92-.174v-.37c0-.93-.105-1.558-.3-1.875-.302-.43-.78-.65-1.44-.65h-.182c-.48.046-.896.196-1.246.46-.35.27-.575.63-.675 1.096-.06.3-.206.465-.435.51l-2.52-.315c-.248-.06-.372-.18-.372-.39 0-.046.007-.09.022-.15.247-1.29.855-2.25 1.82-2.88.976-.616 2.1-.975 3.39-1.05h.54c1.65 0 2.957.434 3.888 1.29.135.15.27.3.405.48.12.165.224.314.283.45.075.134.15.33.195.57.06.254.105.42.135.51.03.104.062.3.076.615.01.313.02.493.02.553v5.28c0 .376.06.72.165 1.036.105.313.21.54.315.674l.51.674c.09.136.136.256.136.36 0 .12-.06.226-.18.314-1.2 1.05-1.86 1.62-1.963 1.71-.165.135-.375.15-.63.045a6.062 6.062 0 01-.526-.496l-.31-.347a9.391 9.391 0 01-.317-.42l-.3-.435c-.81.886-1.603 1.44-2.4 1.665-.494.15-1.093.227-1.83.227-1.11 0-2.04-.343-2.76-1.034-.72-.69-1.08-1.665-1.08-2.94l-.05-.076zm3.753-.438c0 .566.14 1.02.425 1.364.285.34.675.512 1.155.512.045 0 .106-.007.195-.02.09-.016.134-.023.166-.023.614-.16 1.08-.553 1.424-1.178.165-.28.285-.58.36-.91.09-.32.12-.59.135-.8.015-.195.015-.54.015-1.005v-.54c-.84 0-1.484.06-1.92.18-1.275.36-1.92 1.17-1.92 2.43l-.035-.02zm9.162 7.027c.03-.06.075-.11.132-.17.362-.243.714-.41 1.05-.5a8.094 8.094 0 011.612-.24c.14-.012.28 0 .41.03.65.06 1.05.168 1.172.33.063.09.099.228.099.39v.15c0 .51-.149 1.11-.424 1.8-.278.69-.664 1.248-1.156 1.68-.073.06-.14.09-.197.09-.03 0-.06 0-.09-.012-.09-.044-.107-.12-.064-.24.54-1.26.806-2.143.806-2.64 0-.15-.03-.27-.087-.344-.145-.166-.55-.257-1.224-.257-.243 0-.533.016-.87.046-.363.045-.7.09-1 .135-.09 0-.148-.014-.18-.044-.03-.03-.036-.047-.02-.077 0-.017.006-.03.02-.063v-.06z" },
    PlatformIcon { name: "Apple Music", hex: "#FA243C", view_size: 24.0, domains: &["music.apple.com"], path: "M23.994 6.124a9.23 9.23 0 00-.24-2.19c-.317-1.31-1.062-2.31-2.18-3.043a5.022 5.022 0 00-1.877-.726 10.496 10.496 0 00-1.564-.15c-.04-.003-.083-.01-.124-.013H5.986c-.152.01-.303.017-.455.026-.747.043-1.49.123-2.193.4-1.336.53-2.3 1.452-2.865 2.78-.192.448-.292.925-.363 1.408-.056.392-.088.785-.1 1.18 0 .032-.007.062-.01.093v12.223c.01.14.017.283.027.424.05.815.154 1.624.497 2.373.65 1.42 1.738 2.353 3.234 2.801.42.127.856.187 1.293.228.555.053 1.11.06 1.667.06h11.03a12.5 12.5 0 001.57-.1c.822-.106 1.596-.35 2.295-.81a5.046 5.046 0 001.88-2.207c.186-.42.293-.87.37-1.324.113-.675.138-1.358.137-2.04-.002-3.8 0-7.595-.003-11.393zm-6.423 3.99v5.712c0 .417-.058.827-.244 1.206-.29.59-.76.962-1.388 1.14-.35.1-.706.157-1.07.173-.95.045-1.773-.6-1.943-1.536a1.88 1.88 0 011.038-2.022c.323-.16.67-.25 1.018-.324.378-.082.758-.153 1.134-.24.274-.063.457-.23.51-.516a.904.904 0 00.02-.193c0-1.815 0-3.63-.002-5.443a.725.725 0 00-.026-.185c-.04-.15-.15-.243-.304-.234-.16.01-.318.035-.475.066-.76.15-1.52.303-2.28.456l-2.325.47-1.374.278c-.016.003-.032.01-.048.013-.277.077-.377.203-.39.49-.002.042 0 .086 0 .13-.002 2.602 0 5.204-.003 7.805 0 .42-.047.836-.215 1.227-.278.64-.77 1.04-1.434 1.233-.35.1-.71.16-1.075.172-.96.036-1.755-.6-1.92-1.544-.14-.812.23-1.685 1.154-2.075.357-.15.73-.232 1.108-.31.287-.06.575-.116.86-.177.383-.083.583-.323.6-.714v-.15c0-2.96 0-5.922.002-8.882 0-.123.013-.25.042-.37.07-.285.273-.448.546-.518.255-.066.515-.112.774-.165.733-.15 1.466-.296 2.2-.444l2.27-.46c.67-.134 1.34-.27 2.01-.403.22-.043.442-.088.663-.106.31-.025.523.17.554.482.008.073.012.148.012.223.002 1.91.002 3.822 0 5.732z" },
    PlatformIcon { name: "Bandcamp", hex: "#408294", view_size: 24.0, domains: &["bandcamp.com"], path: "M0 18.75l7.437-13.5H24l-7.438 13.5H0z" },
    PlatformIcon { name: "Behance", hex: "#1769FF", view_size: 24.0, domains: &["behance.net"], path: "M16.969 16.927a2.561 2.561 0 0 0 1.901.677 2.501 2.501 0 0 0 1.531-.475c.362-.235.636-.584.779-.99h2.585a5.091 5.091 0 0 1-1.9 2.896 5.292 5.292 0 0 1-3.091.88 5.839 5.839 0 0 1-2.284-.433 4.871 4.871 0 0 1-1.723-1.211 5.657 5.657 0 0 1-1.08-1.874 7.057 7.057 0 0 1-.383-2.393c-.005-.8.129-1.595.396-2.349a5.313 5.313 0 0 1 5.088-3.604 4.87 4.87 0 0 1 2.376.563c.661.362 1.231.87 1.668 1.485a6.2 6.2 0 0 1 .943 2.133c.194.821.263 1.666.205 2.508h-7.699c-.063.79.184 1.574.688 2.187ZM6.947 4.084a8.065 8.065 0 0 1 1.928.198 4.29 4.29 0 0 1 1.49.638c.418.303.748.711.958 1.182.241.579.357 1.203.341 1.83a3.506 3.506 0 0 1-.506 1.961 3.726 3.726 0 0 1-1.503 1.287 3.588 3.588 0 0 1 2.027 1.437c.464.747.697 1.615.67 2.494a4.593 4.593 0 0 1-.423 2.032 3.945 3.945 0 0 1-1.163 1.413 5.114 5.114 0 0 1-1.683.807 7.135 7.135 0 0 1-1.928.259H0V4.084h6.947Zm-.235 12.9c.308.004.616-.029.916-.099a2.18 2.18 0 0 0 .766-.332c.228-.158.411-.371.534-.619.142-.317.208-.663.191-1.009a2.08 2.08 0 0 0-.642-1.715 2.618 2.618 0 0 0-1.696-.505h-3.54v4.279h3.471Zm13.635-5.967a2.13 2.13 0 0 0-1.654-.619 2.336 2.336 0 0 0-1.163.259 2.474 2.474 0 0 0-.738.62 2.359 2.359 0 0 0-.396.792c-.074.239-.12.485-.137.734h4.769a3.239 3.239 0 0 0-.679-1.785l-.002-.001Zm-13.813-.648a2.254 2.254 0 0 0 1.423-.433c.399-.355.607-.88.56-1.413a1.916 1.916 0 0 0-.178-.891 1.298 1.298 0 0 0-.495-.533 1.851 1.851 0 0 0-.711-.274 3.966 3.966 0 0 0-.835-.073H3.241v3.631h3.293v-.014ZM21.62 5.122h-5.976v1.527h5.976V5.122Z" },
    PlatformIcon { name: "Bluesky", hex: "#0285FF", view_size: 24.0, domains: &["bsky.app"], path: "M5.202 2.857C7.954 4.922 10.913 9.11 12 11.358c1.087-2.247 4.046-6.436 6.798-8.501C20.783 1.366 24 .213 24 3.883c0 .732-.42 6.156-.667 7.037-.856 3.061-3.978 3.842-6.755 3.37 4.854.826 6.089 3.562 3.422 6.299-5.065 5.196-7.28-1.304-7.847-2.97-.104-.305-.152-.448-.153-.327 0-.121-.05.022-.153.327-.568 1.666-2.782 8.166-7.847 2.97-2.667-2.737-1.432-5.473 3.422-6.3-2.777.473-5.899-.308-6.755-3.369C.42 10.04 0 4.615 0 3.883c0-3.67 3.217-2.517 5.202-1.026" },
    PlatformIcon { name: "Cash App", hex: "#00C244", view_size: 24.0, domains: &["cash.app"], path: "M23.59 3.475a5.1 5.1 0 00-3.05-3.05c-1.31-.42-2.5-.42-4.92-.42H8.36c-2.4 0-3.61 0-4.9.4a5.1 5.1 0 00-3.05 3.06C0 4.765 0 5.965 0 8.365v7.27c0 2.41 0 3.6.4 4.9a5.1 5.1 0 003.05 3.05c1.3.41 2.5.41 4.9.41h7.28c2.41 0 3.61 0 4.9-.4a5.1 5.1 0 003.06-3.06c.41-1.3.41-2.5.41-4.9v-7.25c0-2.41 0-3.61-.41-4.91zm-6.17 4.63l-.93.93a.5.5 0 01-.67.01 5 5 0 00-3.22-1.18c-.97 0-1.94.32-1.94 1.21 0 .9 1.04 1.2 2.24 1.65 2.1.7 3.84 1.58 3.84 3.64 0 2.24-1.74 3.78-4.58 3.95l-.26 1.2a.49.49 0 01-.48.39H9.63l-.09-.01a.5.5 0 01-.38-.59l.28-1.27a6.54 6.54 0 01-2.88-1.57v-.01a.48.48 0 010-.68l1-.97a.49.49 0 01.67 0c.91.86 2.13 1.34 3.39 1.32 1.3 0 2.17-.55 2.17-1.42 0-.87-.88-1.1-2.54-1.72-1.76-.63-3.43-1.52-3.43-3.6 0-2.42 2.01-3.6 4.39-3.71l.25-1.23a.48.48 0 01.48-.38h1.78l.1.01c.26.06.43.31.37.57l-.27 1.37c.9.3 1.75.77 2.48 1.39l.02.02c.19.2.19.5 0 .68z" },
    PlatformIcon { name: "Discord", hex: "#5865F2", view_size: 24.0, domains: &["discord.gg", "discord.com"], path: "M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z" },
    PlatformIcon { name: "Dribbble", hex: "#EA4C89", view_size: 24.0, domains: &["dribbble.com"], path: "M12 24C5.385 24 0 18.615 0 12S5.385 0 12 0s12 5.385 12 12-5.385 12-12 12zm10.12-10.358c-.35-.11-3.17-.953-6.384-.438 1.34 3.684 1.887 6.684 1.992 7.308 2.3-1.555 3.936-4.02 4.395-6.87zm-6.115 7.808c-.153-.9-.75-4.032-2.19-7.77l-.066.02c-5.79 2.015-7.86 6.025-8.04 6.4 1.73 1.358 3.92 2.166 6.29 2.166 1.42 0 2.77-.29 4-.814zm-11.62-2.58c.232-.4 3.045-5.055 8.332-6.765.135-.045.27-.084.405-.12-.26-.585-.54-1.167-.832-1.74C7.17 11.775 2.206 11.71 1.756 11.7l-.004.312c0 2.633.998 5.037 2.634 6.855zm-2.42-8.955c.46.008 4.683.026 9.477-1.248-1.698-3.018-3.53-5.558-3.8-5.928-2.868 1.35-5.01 3.99-5.676 7.17zM9.6 2.052c.282.38 2.145 2.914 3.822 6 3.645-1.365 5.19-3.44 5.373-3.702-1.81-1.61-4.19-2.586-6.795-2.586-.825 0-1.63.1-2.4.285zm10.335 3.483c-.218.29-1.935 2.493-5.724 4.04.24.49.47.985.68 1.486.08.18.15.36.22.53 3.41-.43 6.8.26 7.14.33-.02-2.42-.88-4.64-2.31-6.38z" },
    PlatformIcon { name: "Etsy", hex: "#F16521", view_size: 24.0, domains: &["etsy.com"], path: "M8.559 2.445c0-.325.033-.52.59-.52h7.465c1.3 0 2.02 1.11 2.54 3.193l.42 1.666h1.27c.23-4.728.43-6.784.43-6.784s-3.196.36-5.09.36H6.635L1.521.196v1.37l1.725.326c1.21.24 1.5.496 1.6 1.606 0 0 .11 3.27.11 8.64 0 5.385-.09 8.61-.09 8.61 0 .973-.39 1.333-1.59 1.573l-1.722.33V24l5.13-.165h8.55c1.935 0 6.39.165 6.39.165.105-1.17.75-6.48.855-7.064h-1.2l-1.284 2.91c-1.005 2.28-2.476 2.445-4.11 2.445h-4.906c-1.63 0-2.415-.64-2.415-2.05V12.8s3.62 0 4.79.096c.912.064 1.463.325 1.76 1.598l.39 1.695h1.41l-.09-4.278.192-4.305h-1.391l-.45 1.89c-.283 1.244-.48 1.47-1.754 1.6-1.666.17-4.815.14-4.815.14V2.45h-.05z" },
    PlatformIcon { name: "Facebook", hex: "#0866FF", view_size: 24.0, domains: &["facebook.com", "fb.com"], path: "M9.101 23.691v-7.98H6.627v-3.667h2.474v-1.58c0-4.085 1.848-5.978 5.858-5.978.401 0 .955.042 1.468.103a8.68 8.68 0 0 1 1.141.195v3.325a8.623 8.623 0 0 0-.653-.036 26.805 26.805 0 0 0-.733-.009c-.707 0-1.259.096-1.675.309a1.686 1.686 0 0 0-.679.622c-.258.42-.374.995-.374 1.752v1.297h3.919l-.386 2.103-.287 1.564h-3.246v8.245C19.396 23.238 24 18.179 24 12.044c0-6.627-5.373-12-12-12s-12 5.373-12 12c0 5.628 3.874 10.35 9.101 11.647Z" },
    PlatformIcon { name: "GitHub", hex: "#181717", view_size: 24.0, domains: &["github.com"], path: "M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" },
    PlatformIcon { name: "Instagram", hex: "#FF0069", view_size: 24.0, domains: &["instagram.com"], path: "M7.0301.084c-1.2768.0602-2.1487.264-2.911.5634-.7888.3075-1.4575.72-2.1228 1.3877-.6652.6677-1.075 1.3368-1.3802 2.127-.2954.7638-.4956 1.6365-.552 2.914-.0564 1.2775-.0689 1.6882-.0626 4.947.0062 3.2586.0206 3.6671.0825 4.9473.061 1.2765.264 2.1482.5635 2.9107.308.7889.72 1.4573 1.388 2.1228.6679.6655 1.3365 1.0743 2.1285 1.38.7632.295 1.6361.4961 2.9134.552 1.2773.056 1.6884.069 4.9462.0627 3.2578-.0062 3.668-.0207 4.9478-.0814 1.28-.0607 2.147-.2652 2.9098-.5633.7889-.3086 1.4578-.72 2.1228-1.3881.665-.6682 1.0745-1.3378 1.3795-2.1284.2957-.7632.4966-1.636.552-2.9124.056-1.2809.0692-1.6898.063-4.948-.0063-3.2583-.021-3.6668-.0817-4.9465-.0607-1.2797-.264-2.1487-.5633-2.9117-.3084-.7889-.72-1.4568-1.3876-2.1228C21.2982 1.33 20.628.9208 19.8378.6165 19.074.321 18.2017.1197 16.9244.0645 15.6471.0093 15.236-.005 11.977.0014 8.718.0076 8.31.0215 7.0301.0839m.1402 21.6932c-1.17-.0509-1.8053-.2453-2.2287-.408-.5606-.216-.96-.4771-1.3819-.895-.422-.4178-.6811-.8186-.9-1.378-.1644-.4234-.3624-1.058-.4171-2.228-.0595-1.2645-.072-1.6442-.079-4.848-.007-3.2037.0053-3.583.0607-4.848.05-1.169.2456-1.805.408-2.2282.216-.5613.4762-.96.895-1.3816.4188-.4217.8184-.6814 1.3783-.9003.423-.1651 1.0575-.3614 2.227-.4171 1.2655-.06 1.6447-.072 4.848-.079 3.2033-.007 3.5835.005 4.8495.0608 1.169.0508 1.8053.2445 2.228.408.5608.216.96.4754 1.3816.895.4217.4194.6816.8176.9005 1.3787.1653.4217.3617 1.056.4169 2.2263.0602 1.2655.0739 1.645.0796 4.848.0058 3.203-.0055 3.5834-.061 4.848-.051 1.17-.245 1.8055-.408 2.2294-.216.5604-.4763.96-.8954 1.3814-.419.4215-.8181.6811-1.3783.9-.4224.1649-1.0577.3617-2.2262.4174-1.2656.0595-1.6448.072-4.8493.079-3.2045.007-3.5825-.006-4.848-.0608M16.953 5.5864A1.44 1.44 0 1 0 18.39 4.144a1.44 1.44 0 0 0-1.437 1.4424M5.8385 12.012c.0067 3.4032 2.7706 6.1557 6.173 6.1493 3.4026-.0065 6.157-2.7701 6.1506-6.1733-.0065-3.4032-2.771-6.1565-6.174-6.1498-3.403.0067-6.156 2.771-6.1496 6.1738M8 12.0077a4 4 0 1 1 4.008 3.9921A3.9996 3.9996 0 0 1 8 12.0077" },
    PlatformIcon { name: "Ko-fi", hex: "#FF6433", view_size: 24.0, domains: &["ko-fi.com"], path: "M11.351 2.715c-2.7 0-4.986.025-6.83.26C2.078 3.285 0 5.154 0 8.61c0 3.506.182 6.13 1.585 8.493 1.584 2.701 4.233 4.182 7.662 4.182h.83c4.209 0 6.494-2.234 7.637-4a9.5 9.5 0 0 0 1.091-2.338C21.792 14.688 24 12.22 24 9.208v-.415c0-3.247-2.13-5.507-5.792-5.87-1.558-.156-2.65-.208-6.857-.208m0 1.947c4.208 0 5.09.052 6.571.182 2.624.311 4.13 1.584 4.13 4v.39c0 2.156-1.792 3.844-3.87 3.844h-.935l-.156.649c-.208 1.013-.597 1.818-1.039 2.546-.909 1.428-2.545 3.064-5.922 3.064h-.805c-2.571 0-4.831-.883-6.078-3.195-1.09-2-1.298-4.155-1.298-7.506 0-2.181.857-3.402 3.012-3.714 1.533-.233 3.559-.26 6.39-.26m6.547 2.287c-.416 0-.65.234-.65.546v2.935c0 .311.234.545.65.545 1.324 0 2.051-.754 2.051-2s-.727-2.026-2.052-2.026m-10.39.182c-1.818 0-3.013 1.48-3.013 3.142 0 1.533.858 2.857 1.949 3.897.727.701 1.87 1.429 2.649 1.896a1.47 1.47 0 0 0 1.507 0c.78-.467 1.922-1.195 2.623-1.896 1.117-1.039 1.974-2.364 1.974-3.897 0-1.662-1.247-3.142-3.039-3.142-1.065 0-1.792.545-2.338 1.298-.493-.753-1.246-1.298-2.312-1.298" },
    PlatformIcon { name: "LinkedIn", hex: "#0A66C2", view_size: 448.0, domains: &["linkedin.com"], path: "M416 32L31.9 32C14.3 32 0 46.5 0 64.3L0 447.7C0 465.5 14.3 480 31.9 480L416 480c17.6 0 32-14.5 32-32.3l0-383.4C448 46.5 433.6 32 416 32zM135.4 416l-66.4 0 0-213.8 66.5 0 0 213.8-.1 0zM102.2 96a38.5 38.5 0 1 1 0 77 38.5 38.5 0 1 1 0-77zM384.3 416l-66.4 0 0-104c0-24.8-.5-56.7-34.5-56.7-34.6 0-39.9 27-39.9 54.9l0 105.8-66.4 0 0-213.8 63.7 0 0 29.2 .9 0c8.9-16.8 30.6-34.5 62.9-34.5 67.2 0 79.7 44.3 79.7 101.9l0 117.2z" },
    PlatformIcon { name: "Mastodon", hex: "#6364FF", view_size: 24.0, domains: &["mastodon.social", "mastodon.online", "mstdn.social"], path: "M23.268 5.313c-.35-2.578-2.617-4.61-5.304-5.004C17.51.242 15.792 0 11.813 0h-.03c-3.98 0-4.835.242-5.288.309C3.882.692 1.496 2.518.917 5.127.64 6.412.61 7.837.661 9.143c.074 1.874.088 3.745.26 5.611.118 1.24.325 2.47.62 3.68.55 2.237 2.777 4.098 4.96 4.857 2.336.792 4.849.923 7.256.38.265-.061.527-.132.786-.213.585-.184 1.27-.39 1.774-.753a.057.057 0 0 0 .023-.043v-1.809a.052.052 0 0 0-.02-.041.053.053 0 0 0-.046-.01 20.282 20.282 0 0 1-4.709.545c-2.73 0-3.463-1.284-3.674-1.818a5.593 5.593 0 0 1-.319-1.433.053.053 0 0 1 .066-.054c1.517.363 3.072.546 4.632.546.376 0 .75 0 1.125-.01 1.57-.044 3.224-.124 4.768-.422.038-.008.077-.015.11-.024 2.435-.464 4.753-1.92 4.989-5.604.008-.145.03-1.52.03-1.67.002-.512.167-3.63-.024-5.545zm-3.748 9.195h-2.561V8.29c0-1.309-.55-1.976-1.67-1.976-1.23 0-1.846.79-1.846 2.35v3.403h-2.546V8.663c0-1.56-.617-2.35-1.848-2.35-1.112 0-1.668.668-1.67 1.977v6.218H4.822V8.102c0-1.31.337-2.35 1.011-3.12.696-.77 1.608-1.164 2.74-1.164 1.311 0 2.302.5 2.962 1.498l.638 1.06.638-1.06c.66-.999 1.65-1.498 2.96-1.498 1.13 0 2.043.395 2.74 1.164.675.77 1.012 1.81 1.012 3.12z" },
    PlatformIcon { name: "Medium", hex: "#000000", view_size: 24.0, domains: &["medium.com"], path: "M4.21 0A4.201 4.201 0 0 0 0 4.21v15.58A4.201 4.201 0 0 0 4.21 24h15.58A4.201 4.201 0 0 0 24 19.79v-1.093c-.137.013-.278.02-.422.02-2.577 0-4.027-2.146-4.09-4.832a7.592 7.592 0 0 1 .022-.708c.093-1.186.475-2.241 1.105-3.022a3.885 3.885 0 0 1 1.395-1.1c.468-.237 1.127-.367 1.664-.367h.023c.101 0 .202.004.303.01V4.211A4.201 4.201 0 0 0 19.79 0Zm.198 5.583h4.165l3.588 8.435 3.59-8.435h3.864v.146l-.019.004c-.705.16-1.063.397-1.063 1.254h-.003l.003 10.274c.06.676.424.885 1.063 1.03l.02.004v.145h-4.923v-.145l.019-.005c.639-.144.994-.353 1.054-1.03V7.267l-4.745 11.15h-.261L6.15 7.569v9.445c0 .857.358 1.094 1.063 1.253l.02.004v.147H4.405v-.147l.019-.004c.705-.16 1.065-.397 1.065-1.253V6.987c0-.857-.358-1.094-1.064-1.254l-.018-.004zm19.25 3.668c-1.086.023-1.733 1.323-1.813 3.124H24V9.298a1.378 1.378 0 0 0-.342-.047Zm-1.862 3.632c-.1 1.756.86 3.239 2.204 3.634v-3.634z" },
    PlatformIcon { name: "Patreon", hex: "#000000", view_size: 24.0, domains: &["patreon.com"], path: "M22.957 7.21c-.004-3.064-2.391-5.576-5.191-6.482-3.478-1.125-8.064-.962-11.384.604C2.357 3.231 1.093 7.391 1.046 11.54c-.039 3.411.302 12.396 5.369 12.46 3.765.047 4.326-4.804 6.068-7.141 1.24-1.662 2.836-2.132 4.801-2.618 3.376-.836 5.678-3.501 5.673-7.031Z" },
    PlatformIcon { name: "PayPal", hex: "#003087", view_size: 24.0, domains: &["paypal.com", "paypal.me"], path: "M15.607 4.653H8.941L6.645 19.251H1.82L4.862 0h7.995c3.754 0 6.375 2.294 6.473 5.513-.648-.478-2.105-.86-3.722-.86m6.57 5.546c0 3.41-3.01 6.853-6.958 6.853h-2.493L11.595 24H6.74l1.845-11.538h3.592c4.208 0 7.346-3.634 7.153-6.949a5.24 5.24 0 0 1 2.848 4.686M9.653 5.546h6.408c.907 0 1.942.222 2.363.541-.195 2.741-2.655 5.483-6.441 5.483H8.714Z" },
    PlatformIcon { name: "Pinterest", hex: "#BD081C", view_size: 24.0, domains: &["pinterest.com"], path: "M12.017 0C5.396 0 .029 5.367.029 11.987c0 5.079 3.158 9.417 7.618 11.162-.105-.949-.199-2.403.041-3.439.219-.937 1.406-5.957 1.406-5.957s-.359-.72-.359-1.781c0-1.663.967-2.911 2.168-2.911 1.024 0 1.518.769 1.518 1.688 0 1.029-.653 2.567-.992 3.992-.285 1.193.6 2.165 1.775 2.165 2.128 0 3.768-2.245 3.768-5.487 0-2.861-2.063-4.869-5.008-4.869-3.41 0-5.409 2.562-5.409 5.199 0 1.033.394 2.143.889 2.741.099.12.112.225.085.345-.09.375-.293 1.199-.334 1.363-.053.225-.172.271-.401.165-1.495-.69-2.433-2.878-2.433-4.646 0-3.776 2.748-7.252 7.92-7.252 4.158 0 7.392 2.967 7.392 6.923 0 4.135-2.607 7.462-6.233 7.462-1.214 0-2.354-.629-2.758-1.379l-.749 2.848c-.269 1.045-1.004 2.352-1.498 3.146 1.123.345 2.306.535 3.55.535 6.607 0 11.985-5.365 11.985-11.987C23.97 5.39 18.592.026 11.985.026L12.017 0z" },
    PlatformIcon { name: "Reddit", hex: "#FF4500", view_size: 24.0, domains: &["reddit.com"], path: "M12 0C5.373 0 0 5.373 0 12c0 3.314 1.343 6.314 3.515 8.485l-2.286 2.286C.775 23.225 1.097 24 1.738 24H12c6.627 0 12-5.373 12-12S18.627 0 12 0Zm4.388 3.199c1.104 0 1.999.895 1.999 1.999 0 1.105-.895 2-1.999 2-.946 0-1.739-.657-1.947-1.539v.002c-1.147.162-2.032 1.15-2.032 2.341v.007c1.776.067 3.4.567 4.686 1.363.473-.363 1.064-.58 1.707-.58 1.547 0 2.802 1.254 2.802 2.802 0 1.117-.655 2.081-1.601 2.531-.088 3.256-3.637 5.876-7.997 5.876-4.361 0-7.905-2.617-7.998-5.87-.954-.447-1.614-1.415-1.614-2.538 0-1.548 1.255-2.802 2.803-2.802.645 0 1.239.218 1.712.585 1.275-.79 2.881-1.291 4.64-1.365v-.01c0-1.663 1.263-3.034 2.88-3.207.188-.911.993-1.595 1.959-1.595Zm-8.085 8.376c-.784 0-1.459.78-1.506 1.797-.047 1.016.64 1.429 1.426 1.429.786 0 1.371-.369 1.418-1.385.047-1.017-.553-1.841-1.338-1.841Zm7.406 0c-.786 0-1.385.824-1.338 1.841.047 1.017.634 1.385 1.418 1.385.785 0 1.473-.413 1.426-1.429-.046-1.017-.721-1.797-1.506-1.797Zm-3.703 4.013c-.974 0-1.907.048-2.77.135-.147.015-.241.168-.183.305.483 1.154 1.622 1.964 2.953 1.964 1.33 0 2.47-.81 2.953-1.964.057-.137-.037-.29-.184-.305-.863-.087-1.795-.135-2.769-.135Z" },
    PlatformIcon { name: "Snapchat", hex: "#FFFC00", view_size: 24.0, domains: &["snapchat.com"], path: "M12.206.793c.99 0 4.347.276 5.93 3.821.529 1.193.403 3.219.299 4.847l-.003.06c-.012.18-.022.345-.03.51.075.045.203.09.401.09.3-.016.659-.12 1.033-.301.165-.088.344-.104.464-.104.182 0 .359.029.509.09.45.149.734.479.734.838.015.449-.39.839-1.213 1.168-.089.029-.209.075-.344.119-.45.135-1.139.36-1.333.81-.09.224-.061.524.12.868l.015.015c.06.136 1.526 3.475 4.791 4.014.255.044.435.27.42.509 0 .075-.015.149-.045.225-.24.569-1.273.988-3.146 1.271-.059.091-.12.375-.164.57-.029.179-.074.36-.134.553-.076.271-.27.405-.555.405h-.03c-.135 0-.313-.031-.538-.074-.36-.075-.765-.135-1.273-.135-.3 0-.599.015-.913.074-.6.104-1.123.464-1.723.884-.853.599-1.826 1.288-3.294 1.288-.06 0-.119-.015-.18-.015h-.149c-1.468 0-2.427-.675-3.279-1.288-.599-.42-1.107-.779-1.707-.884-.314-.045-.629-.074-.928-.074-.54 0-.958.089-1.272.149-.211.043-.391.074-.54.074-.374 0-.523-.224-.583-.42-.061-.192-.09-.389-.135-.567-.046-.181-.105-.494-.166-.57-1.918-.222-2.95-.642-3.189-1.226-.031-.063-.052-.15-.055-.225-.015-.243.165-.465.42-.509 3.264-.54 4.73-3.879 4.791-4.02l.016-.029c.18-.345.224-.645.119-.869-.195-.434-.884-.658-1.332-.809-.121-.029-.24-.074-.346-.119-1.107-.435-1.257-.93-1.197-1.273.09-.479.674-.793 1.168-.793.146 0 .27.029.383.074.42.194.789.3 1.104.3.234 0 .384-.06.465-.105l-.046-.569c-.098-1.626-.225-3.651.307-4.837C7.392 1.077 10.739.807 11.727.807l.419-.015h.06z" },
    PlatformIcon { name: "SoundCloud", hex: "#FF5500", view_size: 24.0, domains: &["soundcloud.com"], path: "M23.999 14.165c-.052 1.796-1.612 3.169-3.4 3.169h-8.18a.68.68 0 0 1-.675-.683V7.862a.747.747 0 0 1 .452-.724s.75-.513 2.333-.513a5.364 5.364 0 0 1 2.763.755 5.433 5.433 0 0 1 2.57 3.54c.282-.08.574-.121.868-.12.884 0 1.73.358 2.347.992s.948 1.49.922 2.373ZM10.721 8.421c.247 2.98.427 5.697 0 8.672a.264.264 0 0 1-.53 0c-.395-2.946-.22-5.718 0-8.672a.264.264 0 0 1 .53 0ZM9.072 9.448c.285 2.659.37 4.986-.006 7.655a.277.277 0 0 1-.55 0c-.331-2.63-.256-5.02 0-7.655a.277.277 0 0 1 .556 0Zm-1.663-.257c.27 2.726.39 5.171 0 7.904a.266.266 0 0 1-.532 0c-.38-2.69-.257-5.21 0-7.904a.266.266 0 0 1 .532 0Zm-1.647.77a26.108 26.108 0 0 1-.008 7.147.272.272 0 0 1-.542 0 27.955 27.955 0 0 1 0-7.147.275.275 0 0 1 .55 0Zm-1.67 1.769c.421 1.865.228 3.5-.029 5.388a.257.257 0 0 1-.514 0c-.21-1.858-.398-3.549 0-5.389a.272.272 0 0 1 .543 0Zm-1.655-.273c.388 1.897.26 3.508-.01 5.412-.026.28-.514.283-.54 0-.244-1.878-.347-3.54-.01-5.412a.283.283 0 0 1 .56 0Zm-1.668.911c.4 1.268.257 2.292-.026 3.572a.257.257 0 0 1-.514 0c-.241-1.262-.354-2.312-.023-3.572a.283.283 0 0 1 .563 0Z" },
    PlatformIcon { name: "Spotify", hex: "#1ED760", view_size: 24.0, domains: &["spotify.com"], path: "M12 0C5.4 0 0 5.4 0 12s5.4 12 12 12 12-5.4 12-12S18.66 0 12 0zm5.521 17.34c-.24.359-.66.48-1.021.24-2.82-1.74-6.36-2.101-10.561-1.141-.418.122-.779-.179-.899-.539-.12-.421.18-.78.54-.9 4.56-1.021 8.52-.6 11.64 1.32.42.18.479.659.301 1.02zm1.44-3.3c-.301.42-.841.6-1.262.3-3.239-1.98-8.159-2.58-11.939-1.38-.479.12-1.02-.12-1.14-.6-.12-.48.12-1.021.6-1.141C9.6 9.9 15 10.561 18.72 12.84c.361.181.54.78.241 1.2zm.12-3.36C15.24 8.4 8.82 8.16 5.16 9.301c-.6.179-1.2-.181-1.38-.721-.18-.601.18-1.2.72-1.381 4.26-1.26 11.28-1.02 15.721 1.621.539.3.719 1.02.419 1.56-.299.421-1.02.599-1.559.3z" },
    PlatformIcon { name: "Substack", hex: "#FF6719", view_size: 24.0, domains: &["substack.com"], path: "M22.539 8.242H1.46V5.406h21.08v2.836zM1.46 10.812V24L12 18.11 22.54 24V10.812H1.46zM22.54 0H1.46v2.836h21.08V0z" },
    PlatformIcon { name: "Telegram", hex: "#26A5E4", view_size: 24.0, domains: &["t.me", "telegram.org", "telegram.me"], path: "M11.944 0A12 12 0 0 0 0 12a12 12 0 0 0 12 12 12 12 0 0 0 12-12A12 12 0 0 0 12 0a12 12 0 0 0-.056 0zm4.962 7.224c.1-.002.321.023.465.14a.506.506 0 0 1 .171.325c.016.093.036.306.02.472-.18 1.898-.962 6.502-1.36 8.627-.168.9-.499 1.201-.82 1.23-.696.065-1.225-.46-1.9-.902-1.056-.693-1.653-1.124-2.678-1.8-1.185-.78-.417-1.21.258-1.91.177-.184 3.247-2.977 3.307-3.23.007-.032.014-.15-.056-.212s-.174-.041-.249-.024c-.106.024-1.793 1.14-5.061 3.345-.48.33-.913.49-1.302.48-.428-.008-1.252-.241-1.865-.44-.752-.245-1.349-.374-1.297-.789.027-.216.325-.437.893-.663 3.498-1.524 5.83-2.529 6.998-3.014 3.332-1.386 4.025-1.627 4.476-1.635z" },
    PlatformIcon { name: "Threads", hex: "#000000", view_size: 24.0, domains: &["threads.net", "threads.com"], path: "M18.263 11.097c-.03-3.486-1.92-5.586-5.111-5.586-2.13 0-3.922.963-4.863 2.499l2.062 1.438c.535-.843 1.272-1.543 2.628-1.543 1.528 0 2.318.85 2.544 2.431a15 15 0 0 0-2.236-.173c-4.125 0-6.068 1.867-6.068 4.336s1.943 3.99 4.804 3.99c3.139 0 5.013-2.115 5.781-4.735.798.361 1.348 1.204 1.348 2.47 0 3.387-3.907 5.232-7.22 5.232-4.885 0-8.077-3.207-8.077-8.424 0-6.392 4.223-10.487 9.9-10.487 3.808 0 5.69 1.671 6.97 3.914l2.108-1.475C21.44 2.078 18.331 0 13.663 0 6.227 0 1.168 5.277 1.168 12.934c0 7 4.953 11.066 10.856 11.066 4.878 0 9.809-2.846 9.809-7.716 0-2.545-1.46-4.231-3.569-5.187m-6.33 4.855c-1.077 0-2.026-.512-2.026-1.453 0-1.483 1.822-1.934 3.606-1.934.678 0 1.34.045 1.927.173-.422 1.927-1.671 3.215-3.508 3.214Z" },
    PlatformIcon { name: "TikTok", hex: "#000000", view_size: 24.0, domains: &["tiktok.com"], path: "M12.525.02c1.31-.02 2.61-.01 3.91-.02.08 1.53.63 3.09 1.75 4.17 1.12 1.11 2.7 1.62 4.24 1.79v4.03c-1.44-.05-2.89-.35-4.2-.97-.57-.26-1.1-.59-1.62-.93-.01 2.92.01 5.84-.02 8.75-.08 1.4-.54 2.79-1.35 3.94-1.31 1.92-3.58 3.17-5.91 3.21-1.43.08-2.86-.31-4.08-1.03-2.02-1.19-3.44-3.37-3.65-5.71-.02-.5-.03-1-.01-1.49.18-1.9 1.12-3.72 2.58-4.96 1.66-1.44 3.98-2.13 6.15-1.72.02 1.48-.04 2.96-.04 4.44-.99-.32-2.15-.23-3.02.37-.63.41-1.11 1.04-1.36 1.75-.21.51-.15 1.07-.14 1.61.24 1.64 1.82 3.02 3.5 2.87 1.12-.01 2.19-.66 2.77-1.61.19-.33.4-.67.41-1.06.1-1.79.06-3.57.07-5.36.01-4.03-.01-8.05.02-12.07z" },
    PlatformIcon { name: "Tumblr", hex: "#36465D", view_size: 24.0, domains: &["tumblr.com"], path: "M14.563 24c-5.093 0-7.031-3.756-7.031-6.411V9.747H5.116V6.648c3.63-1.313 4.512-4.596 4.71-6.469C9.84.051 9.941 0 9.999 0h3.517v6.114h4.801v3.633h-4.82v7.47c.016 1.001.375 2.371 2.207 2.371h.09c.631-.02 1.486-.205 1.936-.419l1.156 3.425c-.436.636-2.4 1.374-4.156 1.404h-.178l.011.002z" },
    PlatformIcon { name: "Twitch", hex: "#9146FF", view_size: 24.0, domains: &["twitch.tv"], path: "M11.571 4.714h1.715v5.143H11.57zm4.715 0H18v5.143h-1.714zM6 0L1.714 4.286v15.428h5.143V24l4.286-4.286h3.428L22.286 12V0zm14.571 11.143l-3.428 3.428h-3.429l-3 3v-3H6.857V1.714h13.714Z" },
    PlatformIcon { name: "Venmo", hex: "#008CFF", view_size: 24.0, domains: &["venmo.com"], path: "M21.772 13.119c-.267 0-.381-.251-.38-.655 0-.533.121-1.575.712-1.575.267 0 .357.243.357.598 0 .533-.13 1.632-.689 1.632Zm.502-3.377c-1.677 0-2.405 1.285-2.405 2.658 0 1.042.421 1.874 1.693 1.874 1.717 0 2.438-1.406 2.438-2.763 0-1.025-.462-1.769-1.726-1.769Zm-3.833 0c-.558 0-.964.17-1.393.477-.154-.275-.462-.477-.932-.477-.542 0-.947.219-1.247.437l-.04-.364H13.54l-.688 4.354h1.506l.479-3.053c.129-.065.323-.154.518-.154.145 0 .267.049.267.267 0 .056-.016.145-.024.218l-.429 2.722h1.498l.478-3.053c.138-.073.324-.154.51-.154.146 0 .268.049.268.267 0 .056-.017.145-.025.218l-.429 2.722h1.499l.461-2.908c.025-.153.049-.388.049-.549 0-.582-.267-.97-1.037-.97Zm-6.871 0c-.575 0-.98.219-1.287.421l-.017-.348H8.962l-.689 4.354H9.78l.478-3.053c.13-.065.324-.154.518-.154.147 0 .268.049.268.242 0 .081-.024.227-.032.299l-.422 2.666h1.499l.462-2.908c.024-.153.049-.388.049-.549 0-.582-.268-.97-1.03-.97Zm-5.631 1.834c.041-.485.413-.824.697-.824.162 0 .299.097.299.291 0 .404-.713.533-.996.533Zm.843-1.834c-1.604 0-2.382 1.39-2.382 2.698 0 1.01.478 1.817 1.814 1.817.527 0 1.07-.113 1.418-.282l.186-1.26c-.494.25-.874.347-1.271.347-.365 0-.64-.194-.64-.687.826-.008 2.252-.347 2.252-1.453 0-.687-.494-1.18-1.377-1.18Zm-4.239.267c.089.186.146.412.146.743 0 .606-.429 1.494-.777 2.06l-.373-2.989L0 9.969l.705 4.2h1.757c.77-1.01 1.718-2.448 1.718-3.554 0-.347-.073-.622-.235-.889l-1.402.283Z" },
    PlatformIcon { name: "Vimeo", hex: "#1AB7EA", view_size: 24.0, domains: &["vimeo.com"], path: "M23.9765 6.4168c-.105 2.338-1.739 5.5429-4.894 9.6088-3.2679 4.247-6.0258 6.3699-8.2898 6.3699-1.409 0-2.578-1.294-3.553-3.881l-1.9179-7.1138c-.719-2.584-1.488-3.878-2.312-3.878-.179 0-.806.378-1.8809 1.132l-1.129-1.457a315.06 315.06 0 003.501-3.1279c1.579-1.368 2.765-2.085 3.5539-2.159 1.867-.18 3.016 1.1 3.447 3.838.465 2.953.789 4.789.971 5.5069.5389 2.45 1.1309 3.674 1.7759 3.674.502 0 1.256-.796 2.265-2.385 1.004-1.589 1.54-2.797 1.612-3.628.144-1.371-.395-2.061-1.614-2.061-.574 0-1.167.121-1.777.391 1.186-3.8679 3.434-5.7568 6.7619-5.6368 2.4729.06 3.6279 1.664 3.4929 4.7969z" },
    PlatformIcon { name: "WhatsApp", hex: "#25D366", view_size: 24.0, domains: &["wa.me", "whatsapp.com"], path: "M17.472 14.382c-.297-.149-1.758-.867-2.03-.967-.273-.099-.471-.148-.67.15-.197.297-.767.966-.94 1.164-.173.199-.347.223-.644.075-.297-.15-1.255-.463-2.39-1.475-.883-.788-1.48-1.761-1.653-2.059-.173-.297-.018-.458.13-.606.134-.133.298-.347.446-.52.149-.174.198-.298.298-.497.099-.198.05-.371-.025-.52-.075-.149-.669-1.612-.916-2.207-.242-.579-.487-.5-.669-.51-.173-.008-.371-.01-.57-.01-.198 0-.52.074-.792.372-.272.297-1.04 1.016-1.04 2.479 0 1.462 1.065 2.875 1.213 3.074.149.198 2.096 3.2 5.077 4.487.709.306 1.262.489 1.694.625.712.227 1.36.195 1.871.118.571-.085 1.758-.719 2.006-1.413.248-.694.248-1.289.173-1.413-.074-.124-.272-.198-.57-.347m-5.421 7.403h-.004a9.87 9.87 0 01-5.031-1.378l-.361-.214-3.741.982.998-3.648-.235-.374a9.86 9.86 0 01-1.51-5.26c.001-5.45 4.436-9.884 9.888-9.884 2.64 0 5.122 1.03 6.988 2.898a9.825 9.825 0 012.893 6.994c-.003 5.45-4.437 9.884-9.885 9.884m8.413-18.297A11.815 11.815 0 0012.05 0C5.495 0 .16 5.335.157 11.892c0 2.096.547 4.142 1.588 5.945L.057 24l6.305-1.654a11.882 11.882 0 005.683 1.448h.005c6.554 0 11.89-5.335 11.893-11.893a11.821 11.821 0 00-3.48-8.413Z" },
    PlatformIcon { name: "X", hex: "#000000", view_size: 24.0, domains: &["twitter.com", "x.com"], path: "M14.234 10.162 22.977 0h-2.072l-7.591 8.824L7.251 0H.258l9.168 13.343L.258 24H2.33l8.016-9.318L16.749 24h6.993zm-2.837 3.299-.929-1.329L3.076 1.56h3.182l5.965 8.532.929 1.329 7.754 11.09h-3.182z" },
    PlatformIcon { name: "YouTube", hex: "#FF0000", view_size: 24.0, domains: &["youtube.com", "youtu.be"], path: "M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z" },
];

/// Extracts the host from a (possibly un-normalized) url: strips the
/// scheme, takes everything up to the next `/`, drops a leading "www.".
fn extract_host(url: &str) -> String {
    let normalized = normalize_url(url);
    let after_scheme = normalized
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = after_scheme.split('/').next().unwrap_or(after_scheme);
    host.trim_start_matches("www.").to_lowercase()
}

/// Matches a link's host against each platform's known domains - exact
/// match or a subdomain of one (e.g. "open.spotify.com" still matches
/// "spotify.com").
fn detect_platform(url: &str) -> Option<&'static PlatformIcon> {
    let host = extract_host(url);
    PLATFORMS.iter().find(|p| {
        p.domains
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    })
}

/// Perceived luminance (YIQ weighting), 0-255.
fn luminance(hex: &str) -> f64 {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f64;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f64;
    0.299 * r + 0.587 * g + 0.114 * b
}

/// Outline for a brand badge that would otherwise vanish into the dark card.
/// GitHub, X, TikTok, Threads and friends brand in black or near-black, which
/// sits on the #1a1a1a ground with no visible edge. Brighter badges already
/// stand out, so they get no stroke rather than an outline that only adds noise.
fn badge_outline(hex: &str) -> String {
    if luminance(hex) < 60.0 {
        format!(r#" stroke="rgba({PALETTE_INK_RGB},0.35)" stroke-width="1""#)
    } else {
        String::new()
    }
}

/// Simple perceived-luminance check (YIQ-style weighting) to choose a
/// readable icon fill against a given brand color's badge background.
fn readable_icon_fill(hex: &str) -> &'static str {
    if luminance(hex) > 150.0 {
        "#0a001a"
    } else {
        "white"
    }
}

/// Renders a `LinkCard` as a self-contained SVG: avatar, name, bio, then one
/// row per link. Mirrors bizbuz's `render_card_svg` — same dark/green/purple
/// visual language, same dynamic-height-via-cursor technique, extended with
/// a loop over a variable-length list instead of a fixed set of fields.
fn render_card_svg(card: &LinkCard) -> String {
    const WIDTH: u32 = 400;
    const BG: &str = PALETTE_BG;
    const GREEN: &str = PALETTE_GREEN;
    const ROW_HEIGHT: u32 = 56;

    let name = card.name.clone().unwrap_or_else(|| "".to_string());
    // Name baseline. The avatar's bottom edge is cy + r = 170, and a 26px bold
    // name's cap height reaches ~19px above its baseline — at 190 that left
    // about a pixel between them. 216 gives the photo real breathing room;
    // everything below is laid out from `y`, so it all moves down with it.
    let mut y: u32 = 216;
    let mut body = String::new();

    let cx = WIDTH / 2;
    let cy: u32 = 110;
    let r: u32 = 60;
    if let Some(photo) = &card.photo {
        body.push_str(&format!(
            r#"<defs><clipPath id="avatarClip"><circle cx="{cx}" cy="{cy}" r="{r}"/></clipPath></defs>
<image href="data:image/jpeg;base64,{photo}" x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="xMidYMid slice" clip-path="url(#avatarClip)"/>
<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{GREEN}" stroke-width="2"/>
"#,
            cx - r,
            cy - r,
            r * 2,
            r * 2,
        ));
    } else {
        body.push_str(&format!(
            r#"<circle cx="{cx}" cy="{cy}" r="{r}" fill="{GREEN}"/>
<text x="{cx}" y="{}" font-family="sans-serif" font-size="40" font-weight="bold" fill="{PALETTE_ON_GREEN}" text-anchor="middle">{}</text>
"#,
            cy + 14,
            escape_xml(&get_initials(&name)),
        ));
    }

    body.push_str(&format!(
        r#"<text x="{cx}" y="{y}" font-family="sans-serif" font-size="26" font-weight="bold" fill="{PALETTE_MINT}" text-anchor="middle">{}</text>
"#,
        escape_xml(&name),
    ));

    if let Some(bio) = card.bio.as_deref().filter(|s| !s.is_empty()) {
        y += 34;
        let lines = wrap_text(bio, 42, 3);
        // Quotes wrap the whole bio, not each line. Every line used to get its
        // own pair, so a two-line bio read as two separate quotations.
        let last = lines.len().saturating_sub(1);
        for (i, line) in lines.iter().enumerate() {
            let open = if i == 0 { "\u{201C}" } else { "" };
            let close = if i == last { "\u{201D}" } else { "" };
            body.push_str(&format!(
                r#"<text x="{cx}" y="{y}" font-family="sans-serif" font-style="italic" font-size="12" fill="rgba({PALETTE_INK_RGB},0.7)" text-anchor="middle">{open}{}{close}</text>
"#,
                escape_xml(line),
            ));
            y += 17;
        }
    }

    y += 40;
    let row_x: u32 = 40;
    let row_width: u32 = WIDTH - row_x * 2;
    for entry in card.links.iter().take(MAX_LINKS) {
        let label = if entry.label.trim().is_empty() {
            entry.url.trim().to_string()
        } else {
            entry.label.clone()
        };
        if entry.url.trim().is_empty() {
            continue;
        }
        let href = normalize_url(entry.url.trim());
        let platform = detect_platform(&href);
        // Centre on the drawn row, not the slot. The rect is ROW_HEIGHT - 12
        // tall (the 12 is the gap to the next row) and the icon badge is
        // centred on that — centring the text on the full ROW_HEIGHT put the
        // label 6px below the icon's centre line.
        let text_y = y + (ROW_HEIGHT - 12) / 2 + 5;

        let mut icon_svg = String::new();
        let label_x = if let Some(p) = platform {
            const BADGE: f64 = 28.0;
            const PADDING: f64 = 4.0;
            let badge_x = row_x as f64 + 8.0;
            let badge_y = y as f64 + 8.0;
            let icon_area = BADGE - PADDING * 2.0;
            let scale = icon_area / p.view_size;
            icon_svg = format!(
                r#"<rect x="{badge_x}" y="{badge_y}" width="{BADGE}" height="{BADGE}" rx="8" fill="{}"{}/><g transform="translate({},{}) scale({scale})"><title>{}</title><path d="{}" fill="{}"/></g>"#,
                p.hex,
                badge_outline(p.hex),
                badge_x + PADDING,
                badge_y + PADDING,
                escape_xml(p.name),
                p.path,
                readable_icon_fill(p.hex),
            );
            row_x + 8 + BADGE as u32 + 10
        } else {
            row_x + 18
        };
        let max_label_chars = if platform.is_some() { 22 } else { 28 };
        let display_label = truncate_label(&label, max_label_chars);

        body.push_str(&format!(
            r#"<a href="{}"><rect x="{row_x}" y="{y}" width="{row_width}" height="{}" rx="12" fill="rgba({PALETTE_INK_RGB},0.04)" stroke="rgba({PALETTE_INK_RGB},0.12)"/>{icon_svg}<text x="{label_x}" y="{text_y}" font-family="sans-serif" font-size="15" font-weight="600" fill="rgba({PALETTE_INK_RGB},0.92)">{}</text><text x="{}" y="{text_y}" font-family="sans-serif" font-size="15" fill="{PALETTE_MINT}" text-anchor="end">&#8594;</text></a>
"#,
            escape_xml(&href),
            ROW_HEIGHT - 12,
            escape_xml(&display_label),
            row_x + row_width - 18,
        ));
        y += ROW_HEIGHT + 12;
    }

    y += 12;
    body.push_str(&offering_footer(cx, y));

    let height = y + 24;

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" viewBox="0 0 {WIDTH} {height}"><rect x="0" y="0" width="{WIDTH}" height="{height}" fill="{BG}"/>{body}</svg>"#
    )
}

// ── Link import ──────────────────────────────────────────────────────────────

// Linktree's own human label for a socialLinks[].type value (e.g. "X",
// "TIKTOK") — covers every type actually observed in the wild; unrecognized
// future types fall back to a title-cased rendering of the raw type string
// (parse_social_type below) rather than being dropped.
fn social_type_label(social_type: &str) -> Option<&'static str> {
    Some(match social_type {
        "INSTAGRAM" => "Instagram",
        "TIKTOK" => "TikTok",
        "FACEBOOK" => "Facebook",
        "YOUTUBE" => "YouTube",
        "X" | "TWITTER" => "X",
        "SNAPCHAT" => "Snapchat",
        "THREADS" => "Threads",
        "PINTEREST" => "Pinterest",
        "TWITCH" => "Twitch",
        "SPOTIFY" => "Spotify",
        "SOUNDCLOUD" => "SoundCloud",
        "LINKEDIN" => "LinkedIn",
        "DISCORD" => "Discord",
        "TELEGRAM" => "Telegram",
        "WHATSAPP" => "WhatsApp",
        "EMAIL" => "Email",
        "WEBSITE" => "Website",
        _ => return None,
    })
}

fn title_case_social_type(social_type: &str) -> String {
    social_type
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_linktree(html: &str) -> Vec<LinkEntry> {
    // Locate the opening tag by its id, then advance past its closing `>` —
    // not a fixed literal tag string, since Linktree's other attributes on
    // this tag (e.g. `crossorigin="anonymous"`) vary and would silently
    // break a hardcoded `<script id="__NEXT_DATA__" type="application/json">` match.
    const MARKER: &str = r#"id="__NEXT_DATA__""#;
    let Some(marker_pos) = html.find(MARKER) else { return Vec::new() };
    let Some(tag_end) = html[marker_pos..].find('>') else { return Vec::new() };
    let json_start = marker_pos + tag_end + 1;
    let Some(end) = html[json_start..].find("</script>") else { return Vec::new() };
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&html[json_start..json_start + end]) else {
        return Vec::new();
    };
    // Targets the exact path — deliberately NOT a generic JSON walk, so the
    // unrelated statsigInitValues sponsor-offers array (same {title,url}
    // shape, sibling path) is never visited.
    let main_links = data
        .pointer("/props/pageProps/links")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|l| {
            let url = l.get("url")?.as_str()?.trim().to_string();
            if url.is_empty() {
                return None;
            }
            let label = l
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            Some(LinkEntry { id: String::new(), label, url })
        });

    // The row of icon-only social buttons (Instagram, TikTok, etc.) is a
    // SEPARATE array from the main link list above — easy to miss since a
    // profile with only socials configured looks link-less otherwise.
    let social_links = data
        .pointer("/props/pageProps/socialLinks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| {
            let url = s.get("url")?.as_str()?.trim().to_string();
            if url.is_empty() {
                return None;
            }
            let social_type = s.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let label = social_type_label(social_type)
                .map(|l| l.to_string())
                .unwrap_or_else(|| title_case_social_type(social_type));
            Some(LinkEntry { id: String::new(), label, url })
        });

    main_links.chain(social_links).collect()
}

fn is_hidden(el: &dom_query::Selection) -> bool {
    let style_hidden = el
        .attr("style")
        .map(|s| s.to_lowercase().replace(' ', "").contains("display:none"))
        .unwrap_or(false);
    let class_hidden = el
        .attr("class")
        .map(|c| c.split_whitespace().any(|cls| cls == "d-none" || cls == "hidden"))
        .unwrap_or(false);
    style_hidden || class_hidden
}

fn parse_generic_html(html: &str, page_url: &str) -> Vec<LinkEntry> {
    let doc = dom_query::Document::from(html);
    let page_host = extract_host(page_url);
    // reqwest::Url re-exports url::Url — no separate `url` crate dependency needed.
    let base = reqwest::Url::parse(page_url).ok();

    doc.select("a[href]")
        .iter()
        .filter_map(|a| {
            if is_hidden(&a) {
                return None;
            }
            let href = a.attr("href")?.to_string();
            let href = href.trim();
            if href.is_empty() || href.starts_with('#') {
                return None;
            }
            let absolute_url = base.as_ref()?.join(href).ok()?;
            if absolute_url.scheme() != "http" && absolute_url.scheme() != "https" {
                return None; // drops mailto:, tel:, sms:, javascript:, etc.
            }
            let absolute = absolute_url.to_string();
            let absolute_lower = absolute.to_lowercase();
            if extract_host(&absolute) == page_host {
                return None; // same-origin nav, not an outbound link
            }
            if absolute_lower.contains(&page_host) {
                // Social share widgets (Facebook/Twitter/WhatsApp/LinkedIn/etc.
                // "share this page" buttons) point at a different domain but
                // re-embed the page's own URL as a query param (e.g.
                // "facebook.com/sharer.php?u=https://lnk.bio/x") — that
                // self-reference is what distinguishes them from a real
                // outbound link, regardless of which share service is used.
                return None;
            }
            let label = a
                .attr("title")
                .map(|t| t.to_string())
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| a.text().to_string());
            Some(LinkEntry {
                id: String::new(),
                label: label.trim().to_string(),
                url: absolute,
            })
        })
        .collect()
}

fn dedupe_and_cap(entries: Vec<LinkEntry>, cap: usize) -> Vec<LinkEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|e| seen.insert(e.url.clone()))
        .take(cap)
        .collect()
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
async fn load_cards(app: tauri::AppHandle) -> Result<Vec<LinkCard>, String> {
    Ok(read_cards(&app)?.cards)
}

/// Upserts a card by id. Assigns a fresh id if the card doesn't have one yet
/// (a new card). Rejects new cards once MAX_CARDS is reached. `id`s on
/// new/edited link entries with no id are assigned here so the frontend
/// never has to invent its own scheme.
#[tauri::command]
async fn save_card(app: tauri::AppHandle, mut card: LinkCard) -> Result<LinkCard, String> {
    let mut store = read_cards(&app)?;
    let is_new = card.id.is_empty() || !store.cards.iter().any(|c| c.id == card.id);

    if is_new {
        if store.cards.len() >= MAX_CARDS {
            return Err(format!("You can only keep up to {} cards.", MAX_CARDS));
        }
        if card.id.is_empty() {
            card.id = new_id();
        }
    }

    card.links.truncate(MAX_LINKS);
    for entry in card.links.iter_mut() {
        if entry.id.is_empty() {
            entry.id = new_id();
        }
    }

    store.cards.retain(|c| c.id != card.id);
    store.cards.push(card.clone());
    write_cards(&app, &store)?;
    Ok(card)
}

#[tauri::command]
async fn delete_card(app: tauri::AppHandle, id: String) -> Result<(), String> {
    // Remote first, and local state is kept if it fails. Deleting locally on a
    // failed unpublish would discard the uuid and keypair that are the only
    // route back to the published record, leaving it public permanently. A
    // retryable error beats a permanent orphan; the cost is that deleting a
    // published card needs a connection.
    {
        let store = read_cards(&app)?;
        let card = store
            .cards
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| "Card not found".to_string())?;
        let failed = unpublish_everywhere(&app, &id, BDO_HASH, &card.bdo_uuid_by_env).await;
        if !failed.is_empty() {
            return Err(format!(
                "Couldn't remove the published copy of this card from {}. \
                 It's still here, so nothing was lost — check your connection and try again.",
                failed.join(", ")
            ));
        }
    }

    let mut store = read_cards(&app)?;
    store.cards.retain(|c| c.id != id);
    write_cards(&app, &store)?;

    // The keypair's only purpose was signing for a record that no longer
    // exists; keeping it would leave a usable credential behind.
    let mut keys = read_bdo_keys(&app);
    if keys.remove(&id).is_some() {
        write_bdo_keys(&app, &keys)?;
    }

    // If this was the card shared with sibling apps, stop sharing it —
    // otherwise BizBuz keeps importing a card that no longer exists here,
    // and the card list's "Shared" badge has nothing left to point at.
    if get_shared_card_id(app.clone()).await? == Some(id) {
        clear_shared_card(&app)?;
    }
    Ok(())
}

/// Publishes (or re-publishes, pushing edits) a card to BDO, embedding a
/// rendered SVG so savage can serve it as a live shareable webpage. The link
/// is available the instant this returns — the savage URL is a locally
/// computed signature, not a server-assigned code to wait on.
#[tauri::command]
async fn publish_card(app: tauri::AppHandle, card_id: String) -> Result<LinkCard, String> {
    let mut store = read_cards(&app)?;
    let index = store
        .cards
        .iter()
        .position(|c| c.id == card_id)
        .ok_or_else(|| "Card not found".to_string())?;

    let sessionless = load_or_create_bdo_sessionless(&app, &card_id)?;
    let client = BDO::new(Some(GATEWAY_BDO_URL.to_string()), Some(sessionless));

    let svg = render_card_svg(&store.cards[index]);
    let mut card_json = serde_json::to_value(&store.cards[index]).map_err(|e| e.to_string())?;
    let card_obj = card_json
        .as_object_mut()
        .ok_or_else(|| "card serialized to non-object".to_string())?;
    card_obj.insert("svg".to_string(), serde_json::Value::String(svg));

    // Themes savage's page chrome — the ground behind the card and the button
    // — to match the card. Without this savage falls back to BizBuz's old
    // colours and the card sits on a near-black page with a bright green
    // button. savage accepts only literal hex and ignores anything else,
    // since these land in a style attribute on a script-free page.
    card_obj.insert(
        "palette".to_string(),
        serde_json::json!({
            "background": PALETTE_BG,
            "accent": PALETTE_GREEN,
            "accentText": PALETTE_ON_GREEN,
        }),
    );

    let existing_uuid = store.cards[index].bdo_uuid_by_env.get(GATEWAY_ENV).cloned();

    let uuid = if let Some(uuid) = existing_uuid {
        client
            .update_bdo(&uuid, BDO_HASH, &card_json, &true)
            .await
            .map_err(|e| e.to_string())?;
        uuid
    } else {
        let user = client
            .create_user(BDO_HASH, &card_json, &true)
            .await
            .map_err(|e| e.to_string())?;
        user.uuid
    };

    let timestamp = unix_now_ms_string();
    let signature = client
        .sessionless
        .sign(format!("{timestamp}{uuid}{BDO_HASH}"))
        .to_hex();
    let share_url = format!(
        "{SAVAGE_URL}user/{uuid}/bdo?timestamp={timestamp}&hash={BDO_HASH}&signature={signature}"
    );

    store.cards[index].bdo_uuid_by_env.insert(GATEWAY_ENV.to_string(), uuid);
    store.cards[index].share_url = Some(share_url);
    store.cards[index].published_at = Some(unix_now_ms_string());
    let updated = store.cards[index].clone();
    write_cards(&app, &store)?;
    Ok(updated)
}

// ── Referral link ────────────────────────────────────────────────────────────
//
// Hosted exactly like a card: an SVG published on its own BDO, wrapped as a
// live webpage by savage. One referral identity per app install (not per
// card), reusing the same per-key BDO identity mechanism as cards
// (`load_or_create_bdo_sessionless`) under the fixed key "referral" rather
// than a card id. Mirrors BizBuz's own referral-link feature exactly.
//
// savage's sanitizer strips <script>/javascript:/data: content from the svg,
// so an automatic redirect to the App Store can't survive being embedded in
// the svg — instead the svg just has a plain "Get Linkitylink" button, a
// real <a href> to the App Store.

const REFERRAL_HASH: &str = "linkitylink-referral";
const APP_STORE_URL: &str = "https://apps.apple.com/app/id6808065892";

/// The line at the foot of every published card: who it's from, linking to
/// the family's apps page. One function so the cards and the invite card
/// can't drift apart. A plain <a href> survives savage's sanitizer, the same
/// way contact links do.
const OFFERING_URL: &str = "https://homeventory.club/apps";

fn offering_footer(cx: impl std::fmt::Display, y: impl std::fmt::Display) -> String {
    format!(
        r#"<a href="{OFFERING_URL}"><text x="{cx}" y="{y}" font-family="sans-serif" font-size="11" fill="rgba({PALETTE_INK_RGB},0.4)" text-anchor="middle">A HomeFront offering</text></a>
"#
    )
}

/// Bump when the invite card's design changes, so links already shared get
/// the new version in place (see get_or_create_referral_link).
const REFERRAL_DESIGN: u32 = 2 + REFERRALS_LIVE as u32;

/// The HomeFront referral program: each month, half of what this app earns
/// is shared with the people whose invite cards get tapped, up to $50 each
/// (savage counts the taps and its referral-payouts script pays them). It
/// pays out through the Stripe account people connect in GetPayed, so it
/// stays off until GetPayed is on the App Store: put its listing here to
/// switch the program on. That also bumps REFERRAL_DESIGN, so every invite
/// card already shared is rewritten to route its button through the counter.
const GETPAYED_APP_STORE_URL: Option<&str> = None;
const REFERRALS_LIVE: bool = GETPAYED_APP_STORE_URL.is_some();

/// Where an invite card's "Get" button points: through savage's tap counter
/// when the program is live (carrying the card's own signed query, so savage
/// can check the card is real), which then redirects to the App Store; and
/// straight to the App Store otherwise.
fn referral_button_href(share_url: &str, uuid: &str) -> String {
    if REFERRALS_LIVE {
        share_url.replacen(&format!("user/{uuid}/bdo?"), &format!("r/{uuid}?"), 1)
    } else {
        APP_STORE_URL.to_string()
    }
}

/// savage's taps-and-payouts endpoint for an invite card.
fn referral_stats_url(share_url: &str, uuid: &str) -> String {
    share_url.replacen(&format!("user/{uuid}/bdo?"), &format!("r/{uuid}/stats?"), 1)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ReferralLink {
    uuid: String,
    share_url: String,
    /// The destination this record was last published with, so a later
    /// connect in getpayed can be detected and the record updated in
    /// place. Absent on links published before referrals named a payee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payout: Option<PayoutDestination>,
    /// The App Store URL baked into the published card's "Get" button.
    /// Tracked because the card is published once and cached forever: links
    /// shared while this was still a placeholder would keep pointing at a
    /// listing that doesn't exist. `None` means published before this was
    /// tracked — i.e. almost certainly the placeholder, so it gets rewritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    app_store_url: Option<String>,
    /// REFERRAL_DESIGN when this record was last written; 0 (absent) means
    /// before designs were versioned, which gets rewritten.
    #[serde(default)]
    design: u32,
}
/// The payout destination getpayed wrote to the shared profile, if the user
/// has connected Stripe there. Best-effort: no destination simply means a
/// referral gets published without a payee, which is the normal case for
/// anyone who hasn't set up payouts. A half-filled pair is discarded — a key
/// with no base to resolve it against is worse than nothing, since it would
/// look like a valid payee and route nowhere.
async fn shared_payout_destination(app: &tauri::AppHandle) -> Option<PayoutDestination> {
    load_canonical_profile(app.clone())
        .await
        .ok()
        .flatten()
        .and_then(|profile| profile.payout)
        .filter(|payout| !payout.pub_key.trim().is_empty() && !payout.addie_url.trim().is_empty())
}

/// The referral record as published: the card itself, plus who to pay if a
/// future payout comes of it. Extra fields are safe — savage renders the
/// `svg` field and ignores everything else.
///
/// Note this record is PUBLIC, and the key is the same one already visible
/// on any invoice getpayed publishes. It identifies a payout destination,
/// not a person, and can't be used to move money on its own.
fn referral_bdo(svg: String, payout: Option<&PayoutDestination>) -> serde_json::Value {
    let mut record = serde_json::json!({ "svg": svg });
    if let Some(destination) = payout {
        let object = record.as_object_mut().expect("referral record is an object");
        // Field names match Addie's payee shape, so settling a payout later
        // is a copy rather than a translation. `processor` says how to read
        // the pair instead of leaving a reader to infer it.
        object.insert(
            "payout".to_string(),
            serde_json::json!({
                "processor": "addie-stripe",
                "pubKey": destination.pub_key,
                "addieURL": destination.addie_url,
            }),
        );
    }
    record
}


fn referral_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("referral.json"))
}

// Keyed by GATEWAY_ENV — same reasoning as LinkCard.bdo_uuid_by_env above, a
// referral link published on one gateway doesn't exist on another.
fn read_referral_links(app: &tauri::AppHandle) -> HashMap<String, ReferralLink> {
    let path = match referral_path(app) {
        Ok(p) => p,
        Err(_) => return HashMap::new(),
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_referral_links(app: &tauri::AppHandle, links: &HashMap<String, ReferralLink>) -> Result<(), String> {
    let path = referral_path(app)?;
    let json = serde_json::to_string_pretty(links).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

/// Renders the referral SVG: two interlocking link ovals (echoing the app's
/// own "linked pages" idea), a tagline, and a "Get Linkitylink" button
/// linking to the App Store.
fn render_referral_svg(app_store_url: &str) -> String {
    const WIDTH: u32 = 400;
    const HEIGHT: u32 = 440;
    const BG: &str = PALETTE_BG;
    const GREEN: &str = PALETTE_GREEN;
    const PURPLE: &str = PALETTE_ACCENT;

    let cx = WIDTH / 2;
    let footer = offering_footer(cx, 400);
    let button_y: u32 = 300;

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
<defs><linearGradient id="markGradient" x1="0%" y1="0%" x2="100%" y2="100%"><stop offset="0%" stop-color="{PALETTE_MINT}"/><stop offset="100%" stop-color="{PURPLE}"/></linearGradient></defs>
<rect x="0" y="0" width="{WIDTH}" height="{HEIGHT}" fill="{BG}"/>
<rect x="{}" y="90" width="90" height="50" rx="25" fill="none" stroke="url(#markGradient)" stroke-width="10"/>
<rect x="{}" y="115" width="90" height="50" rx="25" fill="none" stroke="url(#markGradient)" stroke-width="10"/>
<text x="{cx}" y="210" font-family="sans-serif" font-size="30" font-weight="bold" fill="url(#markGradient)" text-anchor="middle">Linkitylink</text>
<text x="{cx}" y="240" font-family="sans-serif" font-size="14" fill="rgba({PALETTE_INK_RGB},0.7)" text-anchor="middle">All your links, one shareable page.</text>
<text x="{cx}" y="270" font-family="sans-serif" font-size="14" fill="{PURPLE}" text-anchor="middle">You've been invited to try it out.</text>
<a href="{}"><rect x="{}" y="{button_y}" width="260" height="56" rx="16" fill="{GREEN}"/><text x="{cx}" y="{}" font-family="sans-serif" font-size="18" font-weight="bold" fill="{PALETTE_ON_GREEN}" text-anchor="middle">Get Linkitylink</text></a>
{footer}</svg>"#,
        cx - 110,
        cx + 20,
        escape_xml(app_store_url),
        cx - 130,
        button_y + 36,
    )
}

/// Returns this install's referral share URL — publishing an svg to its own
/// fresh BDO and computing the permanent savage URL the first time this is
/// called (mirrors `publish_card` exactly), and reusing the result (from
/// local storage) on every call after that, since the referral svg is
/// static and never needs re-publishing.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferralStatus {
    /// The program is switched on (GetPayed is on the App Store).
    live: bool,
    /// Payouts are set up in GetPayed, so this install can earn.
    connected: bool,
    /// GetPayed's App Store listing, for the "set up payouts" button.
    getpayed_url: Option<String>,
    /// savage's taps-and-payouts endpoint for this install's invite card,
    /// once it has one. Fetched by the page itself.
    stats_url: Option<String>,
}

/// What the share screen should show: an invitation to connect payouts in
/// GetPayed, or the invite link with what it has earned.
#[tauri::command]
async fn referral_status(app: tauri::AppHandle) -> ReferralStatus {
    let connected = shared_payout_destination(&app).await.is_some();
    let links = read_referral_links(&app);
    ReferralStatus {
        live: REFERRALS_LIVE,
        connected,
        getpayed_url: GETPAYED_APP_STORE_URL.map(str::to_string),
        stats_url: links.get(GATEWAY_ENV).map(|l| referral_stats_url(&l.share_url, &l.uuid)),
    }
}

#[tauri::command]
async fn get_or_create_referral_link(app: tauri::AppHandle) -> Result<String, String> {
    let payout = shared_payout_destination(&app).await;

    let mut links = read_referral_links(&app);
    if let Some(link) = links.get(GATEWAY_ENV).cloned() {
        // Nothing changed — hand back the link already in circulation.
        if link.payout == payout && link.app_store_url.as_deref() == Some(APP_STORE_URL) && link.design >= REFERRAL_DESIGN {
            return Ok(link.share_url.clone());
        }

        // Something the published card asserts has changed since: the user
        // connected Stripe (or switched payout identity), or the app got a
        // real App Store listing. Update the record IN PLACE so every copy
        // of the link already out there is corrected, rather than minting a
        // new link the old shares would never point at.
        let sessionless = load_or_create_bdo_sessionless(&app, "referral")?;
        let client = BDO::new(Some(GATEWAY_BDO_URL.to_string()), Some(sessionless));
        let record = referral_bdo(render_referral_svg(&referral_button_href(&link.share_url, &link.uuid)), payout.as_ref());
        client
            .update_bdo(&link.uuid, REFERRAL_HASH, &record, &true)
            .await
            .map_err(|e| e.to_string())?;

        let updated = ReferralLink { payout, app_store_url: Some(APP_STORE_URL.to_string()), design: REFERRAL_DESIGN, ..link };
        let share_url = updated.share_url.clone();
        links.insert(GATEWAY_ENV.to_string(), updated);
        write_referral_links(&app, &links)?;
        return Ok(share_url);
    }

    let sessionless = load_or_create_bdo_sessionless(&app, "referral")?;
    let client = BDO::new(Some(GATEWAY_BDO_URL.to_string()), Some(sessionless));

    let card_json = referral_bdo(render_referral_svg(APP_STORE_URL), payout.as_ref());

    let user = client
        .create_user(REFERRAL_HASH, &card_json, &true)
        .await
        .map_err(|e| e.to_string())?;
    let uuid = user.uuid;

    let timestamp = unix_now_ms_string();
    let signature = client
        .sessionless
        .sign(format!("{timestamp}{uuid}{REFERRAL_HASH}"))
        .to_hex();
    let share_url = format!(
        "{SAVAGE_URL}user/{uuid}/bdo?timestamp={timestamp}&hash={REFERRAL_HASH}&signature={signature}"
    );

    // The button can only route through the tap counter once the card has a
    // uuid and a signed link, so a live program rewrites it straight away.
    if REFERRALS_LIVE {
        let record = referral_bdo(render_referral_svg(&referral_button_href(&share_url, &uuid)), payout.as_ref());
        client
            .update_bdo(&uuid, REFERRAL_HASH, &record, &true)
            .await
            .map_err(|e| e.to_string())?;
    }

    let link = ReferralLink {
        uuid,
        share_url: share_url.clone(),
        payout,
        app_store_url: Some(APP_STORE_URL.to_string()),
        design: REFERRAL_DESIGN,
    };
    links.insert(GATEWAY_ENV.to_string(), link);
    write_referral_links(&app, &links)?;
    Ok(share_url)
}

#[tauri::command]
async fn import_links(url: String) -> Result<Vec<LinkEntry>, String> {
    let target = normalize_url(url.trim());
    let host = extract_host(&target);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 \
             (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
        )
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&target).send().await.map_err(|e| {
        if e.is_timeout() {
            format!("{host} took too long to respond. Try again in a moment.")
        } else {
            format!("Couldn't reach {host}. Check the URL and your connection.")
        }
    })?;

    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(format!(
            "{host} blocks automated access, so it can't be imported directly. Copy your links over manually."
        ));
    }
    if !status.is_success() {
        return Err(format!(
            "{host} returned an error ({}). Double-check the URL.",
            status.as_u16()
        ));
    }

    let html = response.text().await.map_err(|e| e.to_string())?;
    let entries = if host.ends_with("linktr.ee") {
        parse_linktree(&html)
    } else {
        parse_generic_html(&html, &target)
    };
    let entries = dedupe_and_cap(entries, IMPORT_SCRAPE_CAP);

    if entries.is_empty() {
        return Err(format!(
            "No links found on that {host} page. It may use a format we don't support yet."
        ));
    }
    Ok(entries)
}

// ── Testing / data deletion ─────────────────────────────────────────────────
//
// Wipes every local file that carries "who this device is" AND unpublishes
// everything first, so the next launch is indistinguishable from a fresh
// install and nothing the user published is left readable.
//
// Deliberately does NOT touch the App-Group-shared canonical profile
// (`canonical.profile`) — that record is owned jointly with BizBuz and the
// other sibling apps, and wiping it here would silently reset those too. The
// `linkitylink.card` handoff key IS cleared, since that's this app's own.
#[tauri::command]
async fn reset_all_data(app: tauri::AppHandle) -> Result<(), String> {
    // Unpublish before deleting anything local, for the same reason
    // delete_card does: the files below are the only record of what was
    // published and the only keys that can authorise its removal.
    let mut failed: Vec<String> = Vec::new();

    for card in read_cards(&app)?.cards {
        for env_key in unpublish_everywhere(&app, &card.id, BDO_HASH, &card.bdo_uuid_by_env).await {
            let label = card.name.clone().unwrap_or_else(|| "card".into());
            failed.push(format!("{label} ({env_key})"));
        }
    }

    let referral_uuids: HashMap<String, String> = read_referral_links(&app)
        .into_iter()
        .map(|(env_key, link)| (env_key, link.uuid))
        .collect();
    for env_key in unpublish_everywhere(&app, "referral", REFERRAL_HASH, &referral_uuids).await {
        failed.push(format!("referral link ({env_key})"));
    }

    if !failed.is_empty() {
        return Err(format!(
            "Couldn't remove published copies of: {}. Nothing was deleted locally, \
             so you can try again — check your connection first.",
            failed.join(", ")
        ));
    }

    let paths = [
        cards_path(&app)?,
        bdo_keys_path(&app)?,
        referral_path(&app)?,
        legacy_card_path(&app)?,
    ];
    for path in paths {
        match fs::remove_file(&path) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("Failed to remove {}: {}", path.display(), err)),
        }
    }

    // This app's own handoff key, so BizBuz stops importing a card that's gone.
    clear_shared_card(&app)?;

    Ok(())
}

// ── App Group sharing ───────────────────────────────────────────────────────
//
// The App Group holds exactly ONE Linkitylink card, under the fixed key
// `linkitylink.card` — that's the single record BizBuz's "Import from
// Linkitylink" reads. So `card_id` here picks which of the user's local
// cards to expose, and every call replaces whatever was shared before.
//
// This is deliberately a user action (a button on the card view), not an
// automatic sync on save: with up to MAX_CARDS cards, silently sharing
// whichever one happened to be saved last gives the user no say in — and no
// way to see — what sibling apps are reading. `get_shared_card_id` below is
// what lets the UI show which card is currently the shared one.

/// The key under which this app's one shared card lives in the App Group.
const SHARED_CARD_KEY: &str = "linkitylink.card";

#[tauri::command]
async fn share_card_to_app_group(app: tauri::AppHandle, card_id: String) -> Result<(), String> {
    let store = read_cards(&app)?;
    let card = store
        .cards
        .iter()
        .find(|c| c.id == card_id)
        .ok_or_else(|| "Card not found".to_string())?;
    let json = serde_json::to_string(card).map_err(|e| e.to_string())?;
    tauri_plugin_app_group::write_value_sync(&app, SHARED_CARD_KEY, &json)
}

/// Id of the card currently shared with sibling apps, or `None` if nothing
/// is shared yet. Parsed as a loose `Value` rather than a `LinkCard` because
/// all this needs is the id, and a record written by a newer build of the
/// app shouldn't make it fail — same version-skew tolerance as the
/// `BizbuzProfileMirror` below. An empty string is the tombstone written by
/// `clear_shared_card` (the plugin can write but not delete), and parses to
/// `None` here.
#[tauri::command]
async fn get_shared_card_id(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(raw) = tauri_plugin_app_group::read_value_sync(&app, SHARED_CARD_KEY)? else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.get("id").and_then(|id| id.as_str()).map(str::to_string))
        .filter(|id| !id.is_empty()))
}

/// Stops sharing whatever card is in the App Group. The plugin has no
/// delete, so an empty string stands in as a tombstone.
fn clear_shared_card(app: &tauri::AppHandle) -> Result<(), String> {
    tauri_plugin_app_group::write_value_sync(app, SHARED_CARD_KEY, "")
}

/// Permissive mirror of BizBuz's `Profile`/`Social` — every field
/// optional/defaulted so a version skew between the two apps (one ahead of
/// the other on a given phone) never breaks deserialization here.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BizbuzSocialMirror {
    github: Option<String>,
    codeberg: Option<String>,
    linkedin: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BizbuzProfileMirror {
    name: Option<String>,
    title: Option<String>,
    company: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    website: Option<String>,
    location: Option<String>,
    bio: Option<String>,
    social: BizbuzSocialMirror,
    photo: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFromBizbuzResult {
    pub name: Option<String>,
    pub bio: Option<String>,
    pub photo: Option<String>,
    pub links: Vec<LinkEntry>,
    pub skipped_fields: Vec<String>,
}

#[tauri::command]
async fn import_from_bizbuz(app: tauri::AppHandle) -> Result<ImportFromBizbuzResult, String> {
    let raw = tauri_plugin_app_group::read_value_sync(&app, "bizbuz.profile")?.ok_or_else(|| {
        "BizBuz hasn't shared anything yet. Open BizBuz and tap \u{201c}Share to App Group\u{201d} first.".to_string()
    })?;
    let profile: BizbuzProfileMirror =
        serde_json::from_str(&raw).map_err(|e| format!("Couldn't read BizBuz's shared profile: {e}"))?;

    let mut links = Vec::new();
    if let Some(site) = profile.website.filter(|s| !s.is_empty()) {
        links.push(LinkEntry { id: String::new(), label: "Website".into(), url: site });
    }
    if let Some(gh) = profile.social.github.filter(|s| !s.is_empty()) {
        links.push(LinkEntry {
            id: String::new(),
            label: "GitHub".into(),
            url: format!("https://github.com/{gh}"),
        });
    }
    if let Some(cb) = profile.social.codeberg.filter(|s| !s.is_empty()) {
        links.push(LinkEntry {
            id: String::new(),
            label: "Codeberg".into(),
            url: format!("https://codeberg.org/{cb}"),
        });
    }
    if let Some(li) = profile.social.linkedin.filter(|s| !s.is_empty()) {
        links.push(LinkEntry {
            id: String::new(),
            label: "LinkedIn".into(),
            url: format!("https://linkedin.com/in/{li}"),
        });
    }

    let mut skipped = Vec::new();
    for (label, val) in [
        ("title", &profile.title),
        ("company", &profile.company),
        ("email", &profile.email),
        ("phone", &profile.phone),
        ("location", &profile.location),
    ] {
        if val.as_deref().is_some_and(|s| !s.is_empty()) {
            skipped.push(label.to_string());
        }
    }

    Ok(ImportFromBizbuzResult {
        name: profile.name,
        bio: profile.bio,
        photo: profile.photo,
        links,
        skipped_fields: skipped,
    })
}

// ── Canonical profile ───────────────────────────────────────────────────────
//
// A third, independent record — separate from this app's own LinkCard —
// holding "all of the user's information" in one place, shared verbatim
// across every app in group.club.home.front via the App Group plugin. Not
// wired into save_card/publish_card in any way; editing it never touches
// link_card.json.

// Higher than MAX_LINKS(16) since this now covers every profile field, not
// just links.
const MAX_PROFILE_FIELDS: usize = 20;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalField {
    pub slug: String,
    pub name: String,
    pub value: String,
}

/// A standard postal address on the shared Canonical Profile. This app has
/// no UI to view or edit it (Gettit does — see its lib.rs) — the field
/// exists here purely so `save_canonical_profile` below can round-trip it
/// without silently erasing whatever Gettit wrote, since every app that
/// touches Canonical Profile overwrites the whole shared record on save.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    pub street: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub city: String,
    pub state: String,
    pub zip: String,
}

/// Where this person's payouts go.
///
/// The pair travels as a unit on purpose. Once Addie is distributed — which
/// production is — a public key alone is ambiguous: it only means anything
/// resolved against the Addie instance holding that account. Addie's own
/// payee shape carries the same two fields (`pubKey` + `addieURL`, see
/// buildPayeeMetadata and /verify-payee in its stripe processor), so this
/// can be lifted straight into a payout request. Keeping them in one struct
/// also makes it impossible to update half the tuple and route money at a
/// key the wrong base has never heard of.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PayoutDestination {
    pub pub_key: String,
    /// Spelled `addieURL` to match Addie's own field, not Rust convention.
    #[serde(rename = "addieURL")]
    pub addie_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalProfile {
    pub photo: Option<String>,
    #[serde(default)]
    pub fields: Vec<CanonicalField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// idothis-owned. Present here only so linkitylink round-trips it
    /// unchanged on save. None = don't touch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idothis_categories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_zip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idothis_rate_cents: Option<u64>,
    /// getpayed-owned. Present here so linkitylink round-trips it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stripe_connected: Option<bool>,
    /// getpayed-owned. Where payouts land; read here (not written) so a
    /// referral card can name who to pay. Carried forward unchanged on save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout: Option<PayoutDestination>,
    pub updated_at: Option<String>,
}

fn slugify(s: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = true; // drop leading separators
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    slug
}

#[tauri::command]
async fn load_canonical_profile(app: tauri::AppHandle) -> Result<Option<CanonicalProfile>, String> {
    let raw = tauri_plugin_app_group::read_value_sync(&app, "canonical.profile")?;
    match raw {
        // A decode failure (e.g. leftover data from an earlier schema) is
        // treated as "nothing saved yet" rather than a hard error.
        Some(json) => Ok(serde_json::from_str(&json).ok()),
        None => Ok(None),
    }
}

#[tauri::command]
async fn save_canonical_profile(app: tauri::AppHandle, mut profile: CanonicalProfile) -> Result<CanonicalProfile, String> {
    // This app's own form never sends real values for address / idothis
    // fields / stripe status, so always carry forward whatever's already
    // stored rather than overwriting them with the incoming None.
    let existing = load_canonical_profile(app.clone()).await?;
    if let Some(existing) = existing {
        if profile.address.is_none() { profile.address = existing.address; }
        if profile.idothis_categories.is_none() { profile.idothis_categories = existing.idothis_categories; }
        if profile.service_zip.is_none() { profile.service_zip = existing.service_zip; }
        if profile.idothis_rate_cents.is_none() { profile.idothis_rate_cents = existing.idothis_rate_cents; }
        if profile.stripe_connected.is_none() { profile.stripe_connected = existing.stripe_connected; }
        if profile.payout.is_none() { profile.payout = existing.payout; }
    }

    let mut deduped: Vec<CanonicalField> = Vec::new();
    for mut field in profile.fields.into_iter() {
        if field.slug.trim().is_empty() {
            field.slug = slugify(&field.name);
        }
        if field.slug.is_empty() {
            continue;
        }
        deduped.retain(|f| f.slug != field.slug);
        deduped.push(field);
    }
    deduped.truncate(MAX_PROFILE_FIELDS);
    profile.fields = deduped;
    profile.updated_at = Some(unix_now_ms_string());
    let json = serde_json::to_string(&profile).map_err(|e| e.to_string())?;
    tauri_plugin_app_group::write_value_sync(&app, "canonical.profile", &json)?;
    Ok(profile)
}

// ── App entry ────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_share_sheet::init())
        .plugin(tauri_plugin_app_group::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            load_cards,
            save_card,
            delete_card,
            publish_card,
            import_links,
            get_or_create_referral_link,
            referral_status,
            share_card_to_app_group,
            get_shared_card_id,
            reset_all_data,
            import_from_bizbuz,
            load_canonical_profile,
            save_canonical_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running linkitylink");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payout key crosses app boundaries as JSON in the App Group, and
    /// each app keeps its OWN copy of the CanonicalProfile struct — four
    /// copies that have to agree. This pins the contract: a profile written
    /// by getpayed must parse here with the key intact, and saving from
    /// this app must not drop it (the carry-forward in
    /// save_canonical_profile), because this app has no UI for it and
    /// always sends None.
    #[test]
    fn canonical_profile_round_trips_payout_key() {
        // Exactly what getpayed writes, including fields this app knows
        // nothing about, to prove unknown keys don't break the parse.
        let written_by_getpayed = r#"{
            "photo": null,
            "fields": [{"slug":"name","name":"Name","value":"Ada"}],
            "stripeConnected": true,
            "payout": {"pubKey": "02c956a0ea38fbff0f33a538df041b5d135a6411c34f390b2203effefd5160fd2f", "addieURL": "https://dev.8as.world/addie/"},
            "idothisRateCents": 9000,
            "someFutureField": {"nested": true},
            "updatedAt": "1790000000000"
        }"#;

        let profile: CanonicalProfile =
            serde_json::from_str(written_by_getpayed).expect("getpayed's profile should parse here");
        let payout = profile.payout.clone().expect("payout destination should parse");
        assert_eq!(payout.pub_key, "02c956a0ea38fbff0f33a538df041b5d135a6411c34f390b2203effefd5160fd2f");
        // Both halves or neither — a key without its base routes nowhere.
        assert_eq!(payout.addie_url, "https://dev.8as.world/addie/");
        assert_eq!(profile.stripe_connected, Some(true));
        assert_eq!(profile.idothis_rate_cents, Some(9000));

        // Re-serializing keeps the camelCase spelling the other apps read.
        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains("\"pubKey\"") && json.contains("\"addieURL\""), "got {json}");

        // A profile with no key at all must parse too — most users have
        // never connected Stripe.
        let no_key: CanonicalProfile = serde_json::from_str(r#"{"fields":[]}"#).unwrap();
        assert_eq!(no_key.payout, None);
    }

    /// A referral record names its payee only when there is one, and says
    /// how to interpret the key. Whoever settles a referral payout later
    /// reads these fields, so their spelling is a contract.
    #[test]
    fn referral_record_names_the_payee_when_known() {
        let destination = PayoutDestination {
            pub_key: "02abc".to_string(),
            addie_url: "https://dev.8as.world/addie/".to_string(),
        };
        let with_payee = referral_bdo("<svg/>".to_string(), Some(&destination));
        assert_eq!(with_payee["payout"]["pubKey"], "02abc");
        // The base is what makes the key resolvable once Addie is distributed.
        assert_eq!(with_payee["payout"]["addieURL"], "https://dev.8as.world/addie/");
        assert_eq!(with_payee["payout"]["processor"], "addie-stripe");
        // The card itself still has to be there — savage renders this field
        // and ignores the rest.
        assert_eq!(with_payee["svg"], "<svg/>");

        let without = referral_bdo("<svg/>".to_string(), None);
        assert!(without.get("payout").is_none());
        assert_eq!(without["svg"], "<svg/>");
    }

    /// Renders a fully-populated card and writes it out, so the published SVG
    /// can be looked at rather than reasoned about. Colour changes especially
    /// are not reviewable by reading hex constants — the text fills were
    /// white-on-dark, and flipping the ground to glacier white without
    /// flipping them would ship an invisible card.
    ///
    ///   cargo test render_sample -- --nocapture
    ///   rsvg-convert -w 400 /tmp/linkitylink-card-sample.svg -o /tmp/card.png
    #[test]
    fn render_sample_card() {
        let card = LinkCard {
            id: "sample".into(),
            name: Some("Ada Lovelace".into()),
            bio: Some("Analytical engines, mostly. Occasionally poetry.".into()),
            photo: None,
            links: vec![
                LinkEntry { id: "1".into(), label: "Portfolio".into(), url: "https://example.com".into() },
                LinkEntry { id: "2".into(), label: "GitHub".into(), url: "https://github.com/ada".into() },
                LinkEntry { id: "3".into(), label: "Instagram".into(), url: "https://instagram.com/ada".into() },
            ],
            ..Default::default()
        };

        let svg = render_card_svg(&card);
        std::fs::write("/tmp/linkitylink-card-sample.svg", &svg).unwrap();

        assert!(!svg.contains("rgba(255,255,255"), "white text on a light card");
        assert!(svg.contains(PALETTE_BG), "card ground should use the palette");
        println!("wrote /tmp/linkitylink-card-sample.svg ({} bytes)", svg.len());
    }

    #[test]
    fn render_sample_referral() {
        let svg = render_referral_svg(APP_STORE_URL);
        std::fs::write("/tmp/linkitylink-referral-sample.svg", &svg).unwrap();
        assert!(!svg.contains("rgba(255,255,255"), "white text on a light card");
        println!("wrote /tmp/linkitylink-referral-sample.svg ({} bytes)", svg.len());
    }
}

#[cfg(test)]
mod offering_footer_tests {
    use super::*;

    #[test]
    fn invite_card_says_homefront_and_links_to_the_apps_page() {
        let svg = render_referral_svg("https://apps.apple.com/app/id1");
        assert!(svg.contains(r#"<a href="https://homeventory.club/apps">"#), "{svg}");
        assert!(svg.contains(">A HomeFront offering</text></a>"));
        assert!(!svg.contains("Freyja"));
    }

    #[test]
    fn footer_is_a_link() {
        let f = offering_footer(200, 400);
        assert!(f.starts_with(r#"<a href="https://homeventory.club/apps"><text x="200" y="400""#), "{f}");
    }
}

#[cfg(test)]
mod referral_program_tests {
    use super::*;

    const SHARE: &str = "https://prod.8as.world/savage/user/abc-123/bdo?timestamp=1&hash=x-referral&signature=ff";

    #[test]
    fn stats_url_is_the_same_signed_query() {
        assert_eq!(
            referral_stats_url(SHARE, "abc-123"),
            "https://prod.8as.world/savage/r/abc-123/stats?timestamp=1&hash=x-referral&signature=ff"
        );
    }

    #[test]
    fn button_goes_straight_to_the_store_until_the_program_is_live() {
        assert!(!REFERRALS_LIVE, "switch on deliberately, once GetPayed is listed");
        assert_eq!(referral_button_href(SHARE, "abc-123"), APP_STORE_URL);
        assert_eq!(REFERRAL_DESIGN, 2);
    }

    #[test]
    fn a_signed_href_is_escaped_in_the_card() {
        let svg = render_referral_svg("https://prod.8as.world/savage/r/abc-123?timestamp=1&hash=x&signature=ff");
        assert!(svg.contains("?timestamp=1&amp;hash=x&amp;signature=ff"), "{svg}");
    }
}
