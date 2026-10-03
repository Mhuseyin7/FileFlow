// Windows: hide the console when the tauri feature is on (release only).
#![cfg_attr(
    all(not(debug_assertions), feature = "tauri"),
    windows_subsystem = "windows"
)]

fn main() {
    #[cfg(feature = "tauri")]
    run_tauri();
    #[cfg(not(feature = "tauri"))]
    {
        eprintln!(
            "fileflow-app was built without the `tauri` feature. Enable it with:\n\
             \n    cargo build -p fileflow-app --features tauri\n\
             \nThe core rule engine tests run without it via:\n\
             \n    cargo test -p fileflow-core\n"
        );
    }
}

#[cfg(feature = "tauri")]
fn run_tauri() {
    use fileflow_app_lib::commands::tauri_bindings::*;
    use fileflow_app_lib::state::AppState;

    let data_dir = dirs_local_data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("FileFlow");
    let state = AppState::new(data_dir).expect("init AppState");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_rules,
            save_rule,
            delete_rule,
            set_sandbox_roots,
            get_sandbox_roots,
            preview,
            run,
            journal,
            undo,
            start_watcher,
            stop_watcher,
            watcher_running
        ])
        .run(tauri::generate_context!())
        .expect("error while running FileFlow");
}

#[cfg(feature = "tauri")]
fn dirs_local_data_dir() -> Option<std::path::PathBuf> {
    // Small stand-in for the `dirs` crate — we only need the local app-data
    // dir. On Windows it's %LOCALAPPDATA%; on macOS/Linux we use $XDG_DATA_HOME
    // or ~/.local/share.
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
            })
    }
}
