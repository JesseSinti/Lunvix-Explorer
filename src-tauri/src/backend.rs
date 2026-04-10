use chrono::{DateTime, TimeZone, Utc};
use dashmap::DashMap;
use directories::UserDirs;
use jwalk::WalkDir;
use redb::ReadableDatabase;
use redb::ReadableTable;
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::fs::read_dir;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::time::{Duration, UNIX_EPOCH};
use sysinfo::Disks;
use time::{OffsetDateTime, macros::format_description};

pub const ENTRIES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("entries");
pub const TREE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("tree");
pub const META_TABLE: TableDefinition<&str, u64> = TableDefinition::new("metadata");

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub enum FileKind {
    File,
    Directory,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct FileMetaData {
    pub size: u64,
    pub created: Option<u64>,
    pub modified: u64,
    pub accessed: Option<u64>,
    pub readonly: bool,
    pub hidden: bool,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Clone)]
pub struct FileNode {
    pub name: String,
    pub kind: FileKind,
    pub metadata: FileMetaData,
    pub extension: Option<String>,
    pub lowercase_path: String,
}

#[derive(Clone, Copy, Default, PartialEq, Debug, Serialize, Deserialize)]
pub enum SortColumn {
    #[default]
    Name,
    Size,
    Extension,
    ModifiedDate,
    CreatedDate,
}

#[derive(PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
}

pub struct SearchRequest {
    pub file_query: String,
    pub current_dir: PathBuf,
    pub sortcolumn: SortColumn,
    pub sortorder: SortOrder,
    pub pil_cat: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileView {
    pub icon: String,
    pub display_name: String,
    pub lowercase_name: String,
    pub display_size: String,
    pub size_bytes: u64,
    pub display_kind: FileKind,
    pub display_modified: String,
    pub time_modified: u64,
    pub display_created: String,
    pub time_created: u64,
    pub display_ext: Option<String>,
    pub lowercase_ext: String,
    pub full_path: String,
}

pub struct ExplorerState {
    pub entries: Arc<DashMap<PathBuf, FileNode>>,
    pub directory_tree: Arc<DashMap<PathBuf, Vec<PathBuf>>>,
    pub hover_paths: Arc<DashMap<PathBuf, Vec<PathBuf>>>,
    pub pil_type: std::sync::Mutex<String>,
    pub db: Arc<Database>,
    pub current_dir: std::sync::Mutex<PathBuf>,
    pub file_query: std::sync::Mutex<String>,
    pub is_indexing: Arc<AtomicBool>,
    pub watch_tx: std::sync::Mutex<mpsc::Sender<PathBuf>>,
}

#[derive(Serialize)]
pub struct SidebarData {
    pub quick_access: Vec<(String, String)>,
    pub system_disks: Vec<(String, String)>,
}

pub fn insert_entry(state: &ExplorerState, path: PathBuf, node: FileNode) -> anyhow::Result<()> {
    let path = normalize_path(&path);
    let path_str = path.to_string_lossy();

    state.entries.insert(path.clone(), node.clone());

    if let Some(parent) = path.parent() {
        let parent_path = parent.to_path_buf();
        state
            .directory_tree
            .entry(parent_path.clone())
            .and_modify(|tree| {
                tree.retain(|p| {
                    p.to_string_lossy().to_ascii_lowercase()
                        != path.to_string_lossy().to_ascii_lowercase()
                });
                tree.push(path.clone());
            })
            .or_insert(vec![path.clone()]);

        let write_txn = state.db.begin_write()?;
        {
            let mut entry_table = write_txn.open_table(ENTRIES_TABLE)?;
            let mut tree_table = write_txn.open_table(TREE_TABLE)?;

            let node_bytes = bincode::serialize(&node)?;
            entry_table.insert(path_str.as_ref(), node_bytes.as_slice())?;

            let children = state.directory_tree.get(&parent_path).unwrap();
            let tree_bytes = bincode::serialize(&*children)?;
            tree_table.insert(
                parent_path.to_string_lossy().as_ref(),
                tree_bytes.as_slice(),
            )?;
        }
        write_txn.commit()?;
    }
    Ok(())
}

pub fn delete_entry(state: &ExplorerState, path: PathBuf) -> anyhow::Result<()> {
    let path = normalize_path(&path);
    let path_str = path.to_string_lossy();

    if let Some(parent) = path.parent() {
        let parent_path = parent.to_path_buf();

        state.entries.remove(&path);
        if let Some(mut children) = state.directory_tree.get_mut(&parent_path) {
            children.retain(|p| {
                p.to_string_lossy().to_ascii_lowercase()
                    != path.to_string_lossy().to_ascii_lowercase()
            });
        }

        let write_txn = state.db.begin_write()?;
        {
            let mut entry_table = write_txn.open_table(ENTRIES_TABLE)?;
            let mut tree_table = write_txn.open_table(TREE_TABLE)?;

            entry_table.remove(path_str.as_ref())?;

            if let Some(children) = state.directory_tree.get(&parent_path) {
                let tree_bytes = bincode::serialize(&*children)?;
                tree_table.insert(
                    parent_path.to_string_lossy().as_ref(),
                    tree_bytes.as_slice(),
                )?;
            }
        }
        write_txn.commit()?;
    }
    Ok(())
}

pub fn create_node_from_path(path: &PathBuf) -> Option<FileNode> {
    let meta = fs::metadata(path).ok()?;
    let kind = if meta.is_dir() {
        FileKind::Directory
    } else {
        FileKind::File
    };

    let is_hidden = {
        #[cfg(target_os = "windows")]
        {
            (meta.file_attributes() & 0x00000002) != 0
        }
        #[cfg(not(target_os = "windows"))]
        {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .starts_with(".")
        }
    };

    let custom_meta = FileMetaData {
        size: meta.len(),
        created: meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        modified: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0),
        accessed: meta
            .accessed()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        readonly: meta.permissions().readonly(),
        hidden: is_hidden,
    };

    Some(FileNode {
        name: path.file_name()?.to_string_lossy().to_string(),
        kind,
        metadata: custom_meta,
        extension: path.extension().map(|e| e.to_string_lossy().to_string()),
        lowercase_path: path.to_string_lossy().to_ascii_lowercase(),
    })
}

pub fn hidden_status(path: &PathBuf) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        if let Ok(metadata) = fs::metadata(&path) {
            return metadata.file_attributes() & 0x00000002 != 0;
        }
    }
    name.starts_with(".")
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    let clean_path = path_str.strip_prefix(r#"\\?\"#).unwrap_or(&path_str);
    #[cfg(target_os = "windows")]
    {
        let mut s = clean_path.to_string();
        unsafe {
            let bytes = s.as_bytes_mut();
            for b in bytes {
                if *b == b'/' {
                    *b = b'\\';
                }
            }
        }
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            s[0..1].make_ascii_uppercase();
        }
        PathBuf::from(s)
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from(clean_path)
    }
}

pub fn base_directory() -> PathBuf {
    if cfg!(target_os = "windows") {
        PathBuf::from("C:\\")
    } else {
        PathBuf::from("/")
    }
}

pub fn cache_data(
    entries: Arc<DashMap<PathBuf, FileNode>>,
    directory_tree: Arc<DashMap<PathBuf, Vec<PathBuf>>>,
    db: Arc<Database>,
    is_indexing: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    is_indexing.store(true, Ordering::SeqCst);
    let root_drive = normalize_path(&base_directory());

    let write_txn = db.begin_write()?;
    {
        let mut entry_table = write_txn.open_table(ENTRIES_TABLE)?;
        let mut tree_table = write_txn.open_table(TREE_TABLE)?;

        let walker = WalkDir::new(root_drive)
            .follow_links(false)
            .parallelism(jwalk::Parallelism::RayonDefaultPool {
                busy_timeout: Duration::from_secs(0),
            })
            .process_read_dir(|_depth, _path, _state, children| {
                children.retain(|dir_entry_result| {
                    dir_entry_result
                        .as_ref()
                        .map(|entry| {
                            let name = entry.file_name.to_string_lossy();
                            !(entry.file_type.is_dir()
                                && (name == "node_modules"
                                    || name == ".git"
                                    || name == "target"
                                    || name == "dist"
                                    || name == "AppData"))
                        })
                        .unwrap_or(true)
                });
            });

        for entry_result in walker {
            let entry = match entry_result {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.depth == 0 {
                continue;
            }

            let path = entry.path();
            if let Ok(meta) = entry.metadata() {
                let custom_meta = FileMetaData {
                    size: meta.len(),
                    created: meta
                        .created()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    modified: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    accessed: meta
                        .accessed()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                    readonly: meta.permissions().readonly(),
                    hidden: hidden_status(&path),
                };

                let kind = if meta.is_dir() {
                    FileKind::Directory
                } else {
                    FileKind::File
                };
                let file_ext = path
                    .extension()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let node = FileNode {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    kind,
                    metadata: custom_meta,
                    extension: Some(file_ext),
                    lowercase_path: path.to_string_lossy().to_ascii_lowercase(),
                };

                if let Some(parent) = path.parent() {
                    let parent_path = parent.to_path_buf();
                    entries.insert(path.clone(), node.clone());
                    directory_tree
                        .entry(parent_path)
                        .or_default()
                        .push(path.clone());

                    let path_str = path.to_string_lossy();
                    let node_bytes = bincode::serialize(&node)?;
                    entry_table.insert(path_str.as_ref(), node_bytes.as_slice())?;
                }
            }
        }

        for entry in directory_tree.iter() {
            let path_str = entry.key().to_string_lossy();
            let children_bytes = bincode::serialize(entry.value())?;
            tree_table.insert(path_str.as_ref(), children_bytes.as_slice())?;
        }
    }
    write_txn.commit()?;
    is_indexing.store(false, Ordering::SeqCst);
    Ok(())
}

pub fn email_file(path: &str) {
    let mut cmd = match std::env::consts::OS {
        "windows" => {
            let script = format!(
                r#"& {{ $outlook = New-Object -ComObject Outlook.Application; $mail = $outlook.CreateItem(0); $mail.Subject = 'Attached File'; $mail.Attachments.Add('{}'); $mail.Display() }}"#,
                path.replace("/", "\\")
            );
            let mut c = Command::new("powershell");
            c.arg("-Command").arg(&script);
            c
        }
        "macos" => {
            let script = format!(
                r#"tell application "Mail" to make new outgoing message with properties {{visible:true, subject:"Attached File"}} and activate; tell content of result to make new attachment with properties {{file name:"{}" as alias}}"#,
                path
            );
            let mut c = Command::new("osascript");
            c.arg("-e").arg(&script);
            c
        }
        "linux" => {
            let mut c = Command::new("xdg-email");
            c.arg("--attach").arg(path);
            c
        }
        _ => return,
    };
    cmd.spawn().ok();
}

pub fn init_db(db_path: PathBuf) -> anyhow::Result<Database> {
    let db = Database::builder().create(&db_path)?;

    let read_txn = db.begin_read()?;

    let needs_init = read_txn.open_table(ENTRIES_TABLE).is_err();

    drop(read_txn);

    if needs_init {
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(ENTRIES_TABLE)?;
            let _ = write_txn.open_table(TREE_TABLE)?;
            let _ = write_txn.open_table(META_TABLE)?;
        }
        write_txn.commit()?;
        println!("Database initialized for the first time at: {:?}", db_path);
    }

    Ok(db)
}

pub fn reconcile_directory(state: &Arc<ExplorerState>, target_dir: &PathBuf) -> anyhow::Result<()> {
    let mut to_update = Vec::new();
    let mut to_delete = Vec::new();
    let mut current_disk_files = HashSet::new();

    if let Ok(entries) = read_dir(target_dir) {
        for entry in entries.flatten() {
            let path = normalize_path(&entry.path());
            current_disk_files.insert(path.clone());

            if let Ok(disk_meta) = std::fs::metadata(&path) {
                let disk_mtime = disk_meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let needs_update = match state.entries.get(&path) {
                    Some(existing_node) => disk_mtime > existing_node.metadata.modified,
                    None => true,
                };

                if needs_update {
                    if let Some(new_node) = create_node_from_path(&path) {
                        to_update.push((path, new_node));
                    }
                }
            }
        }

        if let Some(cached_children) = state.directory_tree.get(target_dir) {
            for cached_path in cached_children.iter() {
                if !current_disk_files.contains(cached_path) {
                    to_delete.push(cached_path.clone());
                }
            }
        }

        if !to_update.is_empty() || !to_delete.is_empty() {
            let write_txn = state.db.begin_write()?;
            {
                let mut entry_table = write_txn.open_table(ENTRIES_TABLE)?;
                let mut tree_table = write_txn.open_table(TREE_TABLE)?;

                for path in to_delete {
                    let path_str = path.to_string_lossy();
                    let _ = entry_table.remove(path_str.as_ref());
                    state.entries.remove(&path);
                }

                for (path, node) in to_update {
                    let path_str = path.to_string_lossy();
                    if let Ok(node_bytes) = bincode::serialize(&node) {
                        let _ = entry_table.insert(path_str.as_ref(), node_bytes.as_slice());
                    }
                    state.entries.insert(path.clone(), node);
                }

                let mut children_update =
                    state.directory_tree.entry(target_dir.clone()).or_default();
                children_update.clear();
                children_update.extend(current_disk_files.into_iter());

                if let Ok(tree_bytes) = bincode::serialize(&*children_update) {
                    let _ = tree_table
                        .insert(target_dir.to_string_lossy().as_ref(), tree_bytes.as_slice());
                }
            }
            write_txn.commit()?;
        }
    }
    Ok(())
}

pub fn load_directory_to_cache(state: &Arc<ExplorerState>, dir: &PathBuf) -> anyhow::Result<()> {
    let read_txn = state.db.begin_read()?;
    let tree_table = read_txn.open_table(TREE_TABLE)?;
    let entry_table = read_txn.open_table(ENTRIES_TABLE)?;

    let dir_str = dir.to_string_lossy();

    if let Ok(Some(tree_bytes)) = tree_table.get(dir_str.as_ref()) {
        let child_paths: Vec<PathBuf> = bincode::deserialize(tree_bytes.value())?;
        state
            .directory_tree
            .insert(dir.clone(), child_paths.clone());

        for path in child_paths {
            let path_str = path.to_string_lossy();
            if let Ok(Some(node_bytes)) = entry_table.get(path_str.as_ref()) {
                if let Ok(node) = bincode::deserialize::<FileNode>(node_bytes.value()) {
                    state.entries.insert(path.clone(), node);
                }
            }
        }
    }
    Ok(())
}

pub fn fetch_files(
    state: &Arc<ExplorerState>,
    request: &SearchRequest,
) -> Vec<(PathBuf, FileNode)> {
    let query = &request.file_query;
    let pil_type = &state.pil_type.lock().unwrap();
    let pil_type_str = pil_type.as_str();
    let mut search_results = Vec::new();

    if query.is_empty() {
        if let Some(children) = state.directory_tree.get(&request.current_dir) {
            for path in children.iter() {
                let hidden = hidden_status(path);
                if pil_type_str == "Hidden" && !hidden || pil_type_str != "Hidden" && hidden {
                    continue;
                }
                if let Some(node) = state.entries.get(path) {
                    search_results.push((path.clone(), node.clone()));
                }
            }
        } else {
            println!("Memory map is empty for this directory — may still be indexing.");
        }
    } else {
        let query = query.to_ascii_lowercase();
        if let Ok(read_txn) = state.db.begin_read() {
            if let Ok(table) = read_txn.open_table(ENTRIES_TABLE) {
                if let Ok(iter) = table.iter() {
                    for item in iter.flatten() {
                        let (path_str_guard, node_bytes_guard) = item;
                        let path = PathBuf::from(path_str_guard.value());
                        let hidden = hidden_status(&path);

                        if pil_type_str == "Hidden" && !hidden || pil_type_str != "Hidden" && hidden
                        {
                            continue;
                        }
                        if !path_str_guard.value().contains(&query) {
                            continue;
                        }
                        if let Ok(node) = bincode::deserialize::<FileNode>(node_bytes_guard.value())
                        {
                            search_results.push((PathBuf::from(path_str_guard.value()), node));
                        }
                        if search_results.len() > 1000 {
                            break;
                        }
                    }
                }
            }
        }
    }

    match pil_type_str {
        "All" | "Hidden" => search_results,
        "Code" => {
            search_results.retain(|(path_buf, _node)| {
                matches!(
                    path_buf.extension().and_then(|ext| ext.to_str()),
                    Some(
                        "rs" | "py"
                            | "js"
                            | "jsx"
                            | "ts"
                            | "tsx"
                            | "go"
                            | "java"
                            | "cpp"
                            | "c"
                            | "h"
                            | "cs"
                            | "rb"
                            | "php"
                            | "swift"
                            | "kt"
                            | "scala"
                            | "pl"
                            | "sh"
                            | "bash"
                            | "sql"
                            | "json"
                            | "xml"
                            | "yaml"
                            | "yml"
                            | "html"
                            | "htm"
                            | "css"
                            | "scss"
                            | "less"
                    )
                )
            });
            search_results
        }
        "Docs" => {
            search_results.retain(|(path_buf, _node)| {
                matches!(
                    path_buf.extension().and_then(|ext| ext.to_str()),
                    Some(
                        "txt"
                            | "md"
                            | "markdown"
                            | "doc"
                            | "docx"
                            | "pdf"
                            | "rtf"
                            | "odt"
                            | "pages"
                            | "log"
                            | "csv"
                            | "xls"
                            | "xlsx"
                            | "ppt"
                            | "pptx"
                            | "key"
                    )
                )
            });
            search_results
        }
        "Media" => {
            search_results.retain(|(path_buf, _node)| {
                matches!(
                    path_buf.extension().and_then(|ext| ext.to_str()),
                    Some(
                        "jpg"
                            | "jpeg"
                            | "png"
                            | "gif"
                            | "bmp"
                            | "tiff"
                            | "tif"
                            | "webp"
                            | "svg"
                            | "ico"
                            | "heic"
                            | "heif"
                            | "raw"
                            | "psd"
                            | "ai"
                            | "eps"
                            | "mp3"
                            | "wav"
                            | "flac"
                            | "aac"
                            | "m4a"
                            | "wma"
                            | "ogg"
                            | "oga"
                            | "opus"
                            | "alac"
                            | "aiff"
                            | "mid"
                            | "midi"
                            | "mp4"
                            | "m4v"
                            | "m4p"
                            | "mov"
                            | "qt"
                            | "avi"
                            | "wmv"
                            | "flv"
                            | "f4v"
                            | "mkv"
                            | "webm"
                            | "3gp"
                            | "3g2"
                            | "m2ts"
                            | "mts"
                            | "ts"
                            | "vob"
                            | "ogv"
                            | "asf"
                            | "rm"
                            | "rmvb"
                    )
                )
            });
            search_results
        }
        _ => {
            eprintln!("Unknown pil_type filter: {}", pil_type_str);
            search_results
        }
    }
}

pub fn format(
    _state: &ExplorerState,
    _request: &SearchRequest,
    filtered_files: Vec<(PathBuf, FileNode)>,
) -> Vec<FileView> {
    filtered_files
        .into_iter()
        .map(|(path, node)| {
            let ext = node.extension.clone().unwrap_or_default();
            let created_ts = node.metadata.created.unwrap_or(0);
            let system_time = UNIX_EPOCH + Duration::from_secs(created_ts);
            let datetime = OffsetDateTime::from(system_time);
            let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]");
            let display_created = match time::UtcOffset::current_local_offset() {
                Ok(offset) => datetime.to_offset(offset).format(&fmt).unwrap_or_default(),
                Err(_) => datetime.format(&fmt).unwrap_or_default(),
            };

            FileView {
                icon: get_icon(node.kind.clone(), node.extension.as_deref()).to_string(),
                display_name: node.name.clone(),
                lowercase_name: node.name.to_lowercase(),
                display_size: bytes_to_mb(node.metadata.size),
                size_bytes: node.metadata.size,
                display_kind: node.kind,
                display_modified: format_modified_time(node.metadata.modified),
                time_modified: node.metadata.modified,
                display_created,
                time_created: created_ts,
                display_ext: Some(ext.clone()),
                lowercase_ext: ext.to_lowercase(),
                full_path: path.to_string_lossy().to_string(),
            }
        })
        .collect()
}

pub fn bytes_to_mb(size: u64) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1_048_576 {
        format!("{:.2} KB", size as f64 / 1024.0)
    } else if size < 1_073_741_824 {
        format!("{:.2} MB", size as f64 / 1_048_576.0)
    } else {
        format!("{:.2} GB", size as f64 / 1_073_741_824.0)
    }
}

pub fn format_modified_time(date: u64) -> String {
    let datetime: DateTime<Utc> = Utc.timestamp_opt(date as i64, 0).unwrap();
    format!("{}", datetime)
}

pub fn get_icon(kind: FileKind, extension: Option<&str>) -> &'static str {
    if kind == FileKind::Directory {
        return "📁";
    }
    match extension.map(|s| s.to_lowercase()).as_deref() {
        Some("rs") => "🦀",
        Some("py") => "🐍",
        Some("js") | Some("ts") | Some("jsx") | Some("tsx") => "📜",
        Some("c") | Some("cpp") | Some("h") | Some("hpp") | Some("cc") => "⚙️",
        Some("go") => "🐹",
        Some("java") | Some("class") | Some("jar") => "☕",
        Some("rb") => "💎",
        Some("php") => "🐘",
        Some("swift") => "🍎",
        Some("kt") | Some("kts") => "🏗️",
        Some("sh") | Some("bat") | Some("zsh") | Some("fish") => "🐚",
        Some("lua") => "🌙",
        Some("html") | Some("htm") | Some("xhtml") => "🌐",
        Some("css") | Some("scss") | Some("sass") | Some("less") => "🎨",
        Some("json") | Some("yaml") | Some("yml") | Some("toml") | Some("xml") => "🔧",
        Some("sql") | Some("db") | Some("sqlite") => "🗄️",
        Some("graphql") | Some("gql") => "⬢",
        Some("txt") | Some("log") => "📄",
        Some("md") | Some("markdown") => "📝",
        Some("pdf") => "📕",
        Some("doc") | Some("docx") | Some("odt") => "📘",
        Some("xls") | Some("xlsx") | Some("csv") | Some("tsv") => "📊",
        Some("ppt") | Some("pptx") => "📽️",
        Some("rtf") | Some("tex") => "📜",
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("bmp") => "🖼️",
        Some("svg") => "📐",
        Some("mp4") | Some("mkv") | Some("avi") | Some("mov") | Some("webm") => "🎬",
        Some("mp3") | Some("wav") | Some("flac") | Some("ogg") | Some("m4a") => "🎵",
        Some("psd") | Some("ai") | Some("xcf") => "🖌️",
        Some("zip") | Some("tar") | Some("gz") | Some("7z") | Some("rar") | Some("bz2") => "📦",
        Some("exe") | Some("msi") | Some("bin") | Some("app") => "🚀",
        Some("deb") | Some("rpm") | Some("pkg") => "🛠️",
        Some("iso") | Some("img") | Some("dmg") => "💿",
        Some("ttf") | Some("otf") | Some("woff") | Some("woff2") => "🔤",
        Some("lock") => "🔒",
        _ => "📄",
    }
}

#[tauri::command]
pub async fn read_directory(
    state: tauri::State<'_, Arc<ExplorerState>>,
    path: String,
    sort_column: SortColumn,
    sort_order: SortOrder,
    pil_type: String,
    search_query: String,
) -> Result<Vec<FileView>, String> {
    let current_dir = PathBuf::from(&path);
    *state.current_dir.lock().unwrap() = current_dir.clone();
    *state.pil_type.lock().unwrap() = pil_type.clone();
    *state.file_query.lock().unwrap() = search_query.clone();
    let _ = state.watch_tx.lock().unwrap().send(current_dir.clone());

    if let Err(e) = load_directory_to_cache(&state, &current_dir) {
        eprintln!("Cache load error: {}", e);
    }
    let _ = reconcile_directory(&state, &current_dir);

    let request = SearchRequest {
        file_query: search_query,
        current_dir: current_dir.clone(),
        sortcolumn: sort_column,
        sortorder: sort_order,
        pil_cat: pil_type,
    };

    let raw_nodes = fetch_files(&state, &request);
    let view_list = format(&state, &request, raw_nodes);

    let (mut dirs, mut files_only): (Vec<_>, Vec<_>) = view_list
        .into_iter()
        .partition(|f| f.display_kind == FileKind::Directory);

    macro_rules! sort_items {
        ($prop:ident) => {{
            let asc = sort_order == SortOrder::Ascending;
            let cmp = |a: &FileView, b: &FileView| {
                if asc {
                    a.$prop.cmp(&b.$prop)
                } else {
                    b.$prop.cmp(&a.$prop)
                }
            };
            dirs.sort_unstable_by(cmp);
            files_only.sort_unstable_by(cmp);
        }};
    }

    match sort_column {
        SortColumn::Name => sort_items!(lowercase_name),
        SortColumn::Size => sort_items!(size_bytes),
        SortColumn::ModifiedDate => sort_items!(time_modified),
        SortColumn::Extension => sort_items!(lowercase_ext),
        SortColumn::CreatedDate => sort_items!(time_created),
    }

    let mut final_list = Vec::with_capacity(dirs.len() + files_only.len());
    final_list.extend(dirs);
    final_list.extend(files_only);

    Ok(final_list)
}

#[tauri::command]
pub async fn get_sidebar() -> Result<SidebarData, String> {
    let mut quick_access: Vec<(String, String)> = Vec::new();

    if let Some(user_dirs) = UserDirs::new() {
        quick_access.push((
            "🏠 Home".to_string(),
            user_dirs.home_dir().to_string_lossy().to_string(),
        ));
        quick_access.push((
            "🖥️ Desktop".to_string(),
            user_dirs
                .desktop_dir()
                .unwrap_or(user_dirs.home_dir())
                .to_string_lossy()
                .to_string(),
        ));
        quick_access.push((
            "📥 Downloads".to_string(),
            user_dirs
                .download_dir()
                .unwrap_or(user_dirs.home_dir())
                .to_string_lossy()
                .to_string(),
        ));
    }

    let mut system_disks: Vec<(String, String)> = Vec::new();
    let disks = Disks::new_with_refreshed_list();
    for disk in &disks {
        let name = disk.name().to_string_lossy();
        let display_name = if name.is_empty() {
            disk.mount_point().to_string_lossy().to_string()
        } else {
            name.to_string()
        };
        system_disks.push((
            display_name,
            disk.mount_point().to_string_lossy().to_string(),
        ));
    }

    Ok(SidebarData {
        quick_access,
        system_disks,
    })
}

#[tauri::command]
pub async fn delete_files(
    state: tauri::State<'_, Arc<ExplorerState>>,
    paths: Vec<String>,
) -> Result<(), String> {
    let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    trash::delete_all(&path_bufs).map_err(|e| e.to_string())?;
    for path in &path_bufs {
        let _ = delete_entry(&state, path.clone());
    }
    Ok(())
}

#[tauri::command]
pub async fn create_item(
    state: tauri::State<'_, Arc<ExplorerState>>,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    let true_path = PathBuf::from(&path);
    if true_path.exists() {
        return Err("A file or folder with that name already exists".to_string());
    }
    if is_dir {
        std::fs::create_dir_all(&true_path).map_err(|e| e.to_string())?;
    } else {
        std::fs::File::create(&true_path).map_err(|e| e.to_string())?;
    }
    if let Some(node) = create_node_from_path(&true_path) {
        let _ = insert_entry(&state, true_path, node);
    }
    Ok(())
}

#[tauri::command]
pub async fn rename_item(
    state: tauri::State<'_, Arc<ExplorerState>>,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    let old = PathBuf::from(&old_path);
    let new = PathBuf::from(&new_path);
    std::fs::rename(&old, &new).map_err(|e| e.to_string())?;

    let norm_old = normalize_path(&old);
    if let Some(mut node) = state.entries.get(&norm_old).map(|r| r.value().clone()) {
        node.name = new
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        node.lowercase_path = new.to_string_lossy().to_ascii_lowercase();
        let _ = delete_entry(&state, old);
        let _ = insert_entry(&state, new, node);
    }
    Ok(())
}

#[tauri::command]
pub async fn read_file_text(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_in_terminal(path: String) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Err("Path does not exist".to_string());
    }

    let working_dir = if path_buf.is_file() {
        path_buf.parent().ok_or("Could not find parent directory")?
    } else {
        &path_buf
    };

    match std::env::consts::OS {
        "windows" => {
            Command::new("cmd")
                .args(["/C", "start", "cmd", "/K"])
                .current_dir(working_dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        "macos" => {
            Command::new("open")
                .args(["-a", "Terminal", &working_dir.to_string_lossy()])
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        _ => {
            let terminals = [
                ("gnome-terminal", "--working-directory"),
                ("xfce4-terminal", "--working-directory"),
                ("konsole", "--workdir"),
                ("alacritty", "--working-directory"),
            ];
            let mut launched = false;

            for (term, dir_arg) in terminals {
                if Command::new(term)
                    .arg(dir_arg)
                    .arg(working_dir)
                    .spawn()
                    .is_ok()
                {
                    launched = true;
                    break;
                }
            }

            if !launched {
                return Err("No supported terminal emulator found".to_string());
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn email_item(path: String) -> Result<(), String> {
    email_file(&path);
    Ok(())
}
