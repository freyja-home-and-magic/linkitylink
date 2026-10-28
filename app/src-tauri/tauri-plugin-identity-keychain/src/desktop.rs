use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};
use tauri::{plugin::PluginApi, AppHandle, Manager, Runtime};

// Desktop stand-in for the keychain group, for `npm run dev`: one JSON file
// in this app's own data dir. Unlike the iOS keychain group it is NOT
// shared between apps, so on desktop each app has its own identities.
pub struct Keychain<R: Runtime> {
    _app: AppHandle<R>,
    path: PathBuf,
    lock: Mutex<()>,
}

impl<R: Runtime> Keychain<R> {
    fn load(&self) -> BTreeMap<String, String> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, items: &BTreeMap<String, String>) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
        fs::write(&self.path, json).map_err(|e| e.to_string())
    }

    pub fn read(&self, service: &str) -> Result<Option<String>, String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        Ok(self.load().get(service).cloned())
    }

    pub fn add(&self, service: &str, value: &str) -> Result<bool, String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        let mut items = self.load();
        if items.contains_key(service) {
            return Ok(false);
        }
        items.insert(service.to_string(), value.to_string());
        self.save(&items)?;
        Ok(true)
    }

    pub fn update(&self, service: &str, value: &str) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        let mut items = self.load();
        items.insert(service.to_string(), value.to_string());
        self.save(&items)
    }

    pub fn delete(&self, service: &str) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|e| e.to_string())?;
        let mut items = self.load();
        items.remove(service);
        self.save(&items)
    }
}

pub fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> tauri::Result<Keychain<R>> {
    let path = app.path().app_data_dir()?.join("identity-keychain.json");
    Ok(Keychain { _app: app.clone(), path, lock: Mutex::new(()) })
}
