#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;

use backend::*;
use dashmap::DashMap;
use notify_debouncer_full::{DebouncedEvent, new_debouncer, notify::*};
use redb::{ReadableDatabase, ReadableTableMetadata};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{Emitter, Manager};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let (watch_tx, watch_rx) = mpsc::channel::<PathBuf>();

            let base_dir = base_directory();
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            std::fs::create_dir_all(&app_data_dir).expect("Failed to create app data directory");

            let mut db_path = app_data_dir;
            db_path.push("Explorer_cache.redb");

            let db = Arc::new(init_db(db_path).expect("Failed to initialize database"));

            let state = Arc::new(ExplorerState {
                entries: Arc::new(DashMap::new()),
                directory_tree: Arc::new(DashMap::new()),
                hover_paths: Arc::new(DashMap::new()),
                pil_type: std::sync::Mutex::new("All".to_string()),
                db: db.clone(),
                current_dir: std::sync::Mutex::new(base_dir.clone()),
                file_query: std::sync::Mutex::new(String::new()),
                is_indexing: Arc::new(AtomicBool::new(false)),
                watch_tx: std::sync::Mutex::new(watch_tx),
            });

            app.manage(state.clone());

            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut current_watched: Option<PathBuf> = None;

                let emit_handle = app_handle.clone();
                let mut debouncer = new_debouncer(
                    Duration::from_millis(350),
                    None,
                    move |res: std::result::Result<Vec<DebouncedEvent>, Vec<Error>>| {
                        if res.is_ok() {
                            let _ = emit_handle.emit("fs_changed", ());
                        }
                    },
                )
                .expect("Failed to create file watcher");

                loop {
                    let mut next_dir = match watch_rx.recv() {
                        Ok(d) => d,
                        Err(_) => break,
                    };

                    while let Ok(newer) = watch_rx.try_recv() {
                        next_dir = newer;
                    }

                    if let Some(ref old) = current_watched {
                        let _ = debouncer.unwatch(old);
                    }
                    let _ = debouncer.watch(&next_dir, RecursiveMode::NonRecursive);
                    current_watched = Some(next_dir);
                }
            });

            let state_for_load = state.clone();
            std::thread::spawn(move || {
                let current_dir = state_for_load.current_dir.lock().unwrap().clone();

                let has_cache = {
                    let read_txn = state_for_load.db.begin_read().unwrap();
                    match read_txn.open_table(ENTRIES_TABLE) {
                        Ok(table) => table.len().unwrap_or(0) > 0,
                        Err(_) => false,
                    }
                };

                if has_cache {
                    if let Err(e) = load_directory_to_cache(&state_for_load, &current_dir) {
                        eprintln!("Initial directory load error: {}", e);
                    }
                    println!("Loaded initial directory from database cache.");
                } else {
                    println!("No cache found. Beginning full filesystem index...");
                    let _ = cache_data(
                        state_for_load.entries.clone(),
                        state_for_load.directory_tree.clone(),
                        state_for_load.db.clone(),
                        state_for_load.is_indexing.clone(),
                    );
                    println!("Filesystem index complete.");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_directory,
            get_sidebar,
            delete_files,
            create_item,
            rename_item,
            read_file_text,
            open_in_terminal,
            email_item,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Rust Explorer");
}
