use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

tauri::ios_plugin_binding!(init_plugin_identity_keychain);

#[derive(Serialize)]
struct ServiceArgs<'a> {
    service: &'a str,
}

#[derive(Serialize)]
struct WriteArgs<'a> {
    service: &'a str,
    value: &'a str,
}

#[derive(Deserialize)]
struct ReadResponse {
    value: Option<String>,
}

#[derive(Deserialize)]
struct AddResponse {
    added: bool,
}

pub struct Keychain<R: Runtime>(PluginHandle<R>);

unsafe impl<R: Runtime> Send for Keychain<R> {}
unsafe impl<R: Runtime> Sync for Keychain<R> {}

impl<R: Runtime> Keychain<R> {
    pub fn read(&self, service: &str) -> Result<Option<String>, String> {
        let resp: ReadResponse = self
            .0
            .run_mobile_plugin("read", ServiceArgs { service })
            .map_err(|e| e.to_string())?;
        Ok(resp.value)
    }

    /// False when an item was already there (nothing written).
    pub fn add(&self, service: &str, value: &str) -> Result<bool, String> {
        let resp: AddResponse = self
            .0
            .run_mobile_plugin("add", WriteArgs { service, value })
            .map_err(|e| e.to_string())?;
        Ok(resp.added)
    }

    pub fn update(&self, service: &str, value: &str) -> Result<(), String> {
        self.0
            .run_mobile_plugin("update", WriteArgs { service, value })
            .map_err(|e| e.to_string())
    }

    pub fn delete(&self, service: &str) -> Result<(), String> {
        self.0
            .run_mobile_plugin("remove", ServiceArgs { service })
            .map_err(|e| e.to_string())
    }
}

pub fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<Keychain<R>, Box<dyn std::error::Error>> {
    let handle = api.register_ios_plugin(init_plugin_identity_keychain)?;
    Ok(Keychain(handle))
}
