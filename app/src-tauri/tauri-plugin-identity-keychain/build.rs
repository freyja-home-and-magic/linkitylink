const COMMANDS: &[&str] = &["public_identity"];

fn main() {
    // iOS keeps identities in the shared keychain group; everywhere else
    // (desktop dev) a file in the app's data dir stands in.
    tauri_plugin::Builder::new(COMMANDS)
        .ios_path("ios")
        .build();
}
