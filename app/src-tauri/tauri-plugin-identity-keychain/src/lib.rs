//! Sessionless identities shared by every HomeFront app, one per allyabase
//! service ("fount" today; addie, bdo or anything later), kept in the
//! keychain group `PCUFMY9NKZ.club.home.front` so BizBuz, Linkitylink,
//! GetPayed and the rest act as the same person.
//!
//! An identity is a keypair plus the uuid that service gave it on each base
//! (`uuids`, keyed the way the apps key bases, e.g. "prod-8as-world"): the
//! key is the person, the uuid is their record on one deployment. The
//! keychain item holds the whole identity as JSON, accessible after first
//! unlock and not synced to iCloud (it does travel in encrypted backups, so
//! a new phone restored from one keeps it).
//!
//! Private keys stay in Rust: the app's own commands use `get`/`get_or_create`
//! below, and the only command exposed to the web view returns the public
//! half. Every copy of this plugin in the HomeFront apps must stay
//! identical; it lives in each app's src-tauri like tauri-plugin-app-group.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(target_os = "ios")]
mod mobile;
#[cfg(not(target_os = "ios"))]
mod desktop;

#[cfg(target_os = "ios")]
use mobile::Keychain;
#[cfg(not(target_os = "ios"))]
use desktop::Keychain;

/// One service's identity. Hex as the sessionless crates write it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub private_key: String,
    pub pub_key: String,
    /// The service's uuid for this key, per base.
    #[serde(default)]
    pub uuids: BTreeMap<String, String>,
}

/// What the web view may see.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicIdentity {
    pub service: String,
    pub pub_key: String,
    pub uuids: BTreeMap<String, String>,
}

fn valid_service(service: &str) -> Result<(), String> {
    let ok = !service.is_empty()
        && service.len() <= 64
        && service.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if ok { Ok(()) } else { Err(format!("not a service name: {service:?}")) }
}

/// The identity for `service`, if this device has one.
pub fn get<R: Runtime>(app: &tauri::AppHandle<R>, service: &str) -> Result<Option<Identity>, String> {
    valid_service(service)?;
    match app.state::<Keychain<R>>().read(service)? {
        Some(json) => serde_json::from_str(&json).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// The identity for `service`, making it with `make` (which returns a
/// fresh `(private_key, pub_key)`) if there isn't one. Two apps doing this
/// at once can't end up with different keys: the add only succeeds if
/// nothing is there, and the loser reads the winner's.
pub fn get_or_create<R: Runtime>(
    app: &tauri::AppHandle<R>,
    service: &str,
    make: impl FnOnce() -> (String, String),
) -> Result<Identity, String> {
    if let Some(existing) = get(app, service)? {
        return Ok(existing);
    }
    let (private_key, pub_key) = make();
    let fresh = Identity { private_key, pub_key, uuids: BTreeMap::new() };
    let json = serde_json::to_string(&fresh).map_err(|e| e.to_string())?;
    if app.state::<Keychain<R>>().add(service, &json)? {
        Ok(fresh)
    } else {
        get(app, service)?.ok_or_else(|| format!("{service} identity vanished after a conflicting add"))
    }
}

/// Records the uuid `service` gave this identity on `base`.
pub fn set_uuid<R: Runtime>(app: &tauri::AppHandle<R>, service: &str, base: &str, uuid: &str) -> Result<Identity, String> {
    let mut identity = get(app, service)?.ok_or_else(|| format!("no {service} identity on this device"))?;
    if identity.uuids.get(base).map(String::as_str) == Some(uuid) {
        return Ok(identity);
    }
    identity.uuids.insert(base.to_string(), uuid.to_string());
    let json = serde_json::to_string(&identity).map_err(|e| e.to_string())?;
    app.state::<Keychain<R>>().update(service, &json)?;
    Ok(identity)
}

/// Removes `service`'s identity from every HomeFront app on this device.
pub fn delete<R: Runtime>(app: &tauri::AppHandle<R>, service: &str) -> Result<(), String> {
    valid_service(service)?;
    app.state::<Keychain<R>>().delete(service)
}

#[tauri::command]
async fn public_identity<R: Runtime>(app: tauri::AppHandle<R>, service: String) -> Result<Option<PublicIdentity>, String> {
    Ok(get(&app, &service)?.map(|i| PublicIdentity { service, pub_key: i.pub_key, uuids: i.uuids }))
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("identity-keychain")
        .invoke_handler(tauri::generate_handler![public_identity])
        .setup(|app, api| {
            #[cfg(target_os = "ios")]
            let keychain = mobile::init(app, api)?;
            #[cfg(not(target_os = "ios"))]
            let keychain = desktop::init(app, api)?;
            app.manage(keychain);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_round_trip_with_uuids_per_base() {
        let mut uuids = BTreeMap::new();
        uuids.insert("prod-8as-world".to_string(), "u1".to_string());
        let identity = Identity { private_key: "aa".into(), pub_key: "02bb".into(), uuids };
        let json = serde_json::to_string(&identity).unwrap();
        assert!(json.contains("\"privateKey\"") && json.contains("\"pubKey\""), "{json}");
        assert_eq!(serde_json::from_str::<Identity>(&json).unwrap(), identity);
        // An identity written before any uuid was known still reads.
        let bare: Identity = serde_json::from_str(r#"{"privateKey":"aa","pubKey":"02bb"}"#).unwrap();
        assert!(bare.uuids.is_empty());
    }

    #[test]
    fn service_names_are_plain() {
        assert!(valid_service("fount").is_ok());
        assert!(valid_service("addie.v2").is_ok());
        assert!(valid_service("").is_err());
        assert!(valid_service("../x").is_err());
    }
}
