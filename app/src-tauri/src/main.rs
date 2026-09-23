// Desktop entry point.
//
// iOS enters through the `mobile_entry_point` on `run()` in lib.rs and never
// touches this file; without it, though, `tauri dev` and `tauri build` have no
// binary to produce and the bundler fails with "can't open main binary". This
// exists so the same UI can be run on a Mac for a look at layout and copy
// without waiting on a device install.
//
// The App Group and share-sheet plugins both ship desktop stubs, so the parts
// of the app that talk to iOS resolve to no-ops here rather than failing:
// publishing works against the real base, tapping Share does nothing.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    linkitylink_lib::run()
}
