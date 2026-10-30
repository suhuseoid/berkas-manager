pub mod store;

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use store::{FileMeta, KdfCost, Store};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

const PREVIEW_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Default)]
struct AppState { store: Mutex<Option<Store>>, dir: Mutex<PathBuf>, view: Mutex<PathBuf> }

#[derive(Serialize)]
struct Status { exists: bool, unlocked: bool, count: usize, dir: String }

#[derive(Serialize)]
struct Item { id: String, name: String, size: u64, kind: String, added: i64 }

#[derive(Serialize, Clone)]
struct Progress { label: String, file: usize, files: usize, done: u64, total: u64 }

#[derive(Serialize)]
struct AddReport { added: usize, failed: Vec<String>, moved: usize }

type R<T> = Result<T, String>;

fn st(app: &AppHandle) -> State<'_, AppState> { app.state::<AppState>() }

fn blobs_and_meta(app: &AppHandle, id: &str) -> R<(PathBuf, FileMeta)> {
    let s = st(app);
    let g = s.store.lock().unwrap();
    let store = g.as_ref().ok_or("Terkunci.")?;
    let meta = store.get(id).cloned().ok_or("File tidak ditemukan.")?;
    Ok((store.blobs_dir(), meta))
}

fn wipe_view(app: &AppHandle) {
    let v = st(app).view.lock().unwrap().clone();
    let _ = std::fs::remove_dir_all(&v);
}

fn open_with_os(p: &Path) -> R<()> {
    #[cfg(target_os = "macos")] let mut c = std::process::Command::new("open");
    #[cfg(target_os = "windows")] let mut c = std::process::Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))] let mut c = std::process::Command::new("xdg-open");
    c.arg(p).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn status(app: AppHandle) -> Status {
    let s = st(&app);
    let dir = s.dir.lock().unwrap().clone();
    let g = s.store.lock().unwrap();
    Status { exists: Store::exists(&dir), unlocked: g.is_some(), count: g.as_ref().map(|x| x.index.files.len()).unwrap_or(0), dir: dir.display().to_string() }
}

#[tauri::command]
async fn create(app: AppHandle, master: String) -> R<()> {
    let master = Zeroizing::new(master);
    let dir = st(&app).dir.lock().unwrap().clone();
    let s = tauri::async_runtime::spawn_blocking(move || Store::create(&dir, &master, KdfCost::default()))
        .await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    *st(&app).store.lock().unwrap() = Some(s);
    Ok(())
}

#[tauri::command]
async fn unlock(app: AppHandle, master: String) -> R<()> {
    let master = Zeroizing::new(master);
    let dir = st(&app).dir.lock().unwrap().clone();
    let s = tauri::async_runtime::spawn_blocking(move || Store::open(&dir, &master))
        .await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    *st(&app).store.lock().unwrap() = Some(s);
    Ok(())
}

#[tauri::command]
fn lock(app: AppHandle) {
    *st(&app).store.lock().unwrap() = None;
    wipe_view(&app);
}

#[tauri::command]
fn list(app: AppHandle, query: String, kind: String) -> R<Vec<Item>> {
    let s = st(&app);
    let g = s.store.lock().unwrap();
    let store = g.as_ref().ok_or("Terkunci.")?;
    let q = query.to_lowercase();
    let mut v: Vec<Item> = store.index.files.iter()
        .filter(|f| kind == "all" || f.kind == kind || (kind == "doc" && matches!(f.kind.as_str(), "doc" | "text" | "other" | "audio" | "archive")))
        .filter(|f| q.is_empty() || f.name.to_lowercase().contains(&q))
        .map(|f| Item { id: f.id.clone(), name: f.name.clone(), size: f.size, kind: f.kind.clone(), added: f.added })
        .collect();
    v.sort_by(|a, b| b.added.cmp(&a.added).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(v)
}

fn collect(paths: Vec<PathBuf>, out: &mut Vec<PathBuf>) {
    for p in paths {
        if out.len() >= 20_000 { return; }
        let hidden = p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(false);
        if hidden { continue; }
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) { collect(rd.filter_map(|e| e.ok().map(|e| e.path())).collect(), out); }
        } else if p.is_file() {
            out.push(p);
        }
    }
}

fn add_blocking(app: &AppHandle, paths: Vec<PathBuf>, move_original: bool) -> R<AddReport> {
    let mut files = Vec::new();
    collect(paths, &mut files);
    let blobs = {
        let s = st(app); let g = s.store.lock().unwrap();
        g.as_ref().ok_or("Terkunci.")?.blobs_dir()
    };
    let n = files.len();
    let mut rep = AddReport { added: 0, failed: vec![], moved: 0 };
    for (i, f) in files.iter().enumerate() {
        let label = f.file_name().and_then(|x| x.to_str()).unwrap_or("").to_string();
        let mut last = 0u64;
        let res = store::encrypt_file(&blobs, f, |done, total| {
            if done == total || done - last >= 8 * store::CHUNK as u64 {
                last = done;
                let _ = app.emit("progress", Progress { label: format!("Mengamankan {label}"), file: i + 1, files: n, done, total });
            }
        });
        match res {
            Ok(meta) => {
                let inserted = {
                    let s = st(app); let mut g = s.store.lock().unwrap();
                    match g.as_mut() { Some(store) => store.insert(meta).map_err(|e| e.to_string()), None => Err("Terkunci.".into()) }
                };
                match inserted {
                    Ok(()) => {
                        rep.added += 1;
                        if move_original && std::fs::remove_file(f).is_ok() { rep.moved += 1; }
                    }
                    Err(e) => rep.failed.push(format!("{label}: {e}")),
                }
            }
            Err(e) => rep.failed.push(format!("{label}: {e}")),
        }
    }
    let _ = app.emit("progress-done", ());
    Ok(rep)
}

#[tauri::command]
async fn add_paths(app: AppHandle, paths: Vec<String>, move_original: bool) -> R<AddReport> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    tauri::async_runtime::spawn_blocking(move || add_blocking(&app, paths, move_original)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn add_dialog(app: AppHandle, move_original: bool, folder: bool) -> R<Option<AddReport>> {
    tauri::async_runtime::spawn_blocking(move || {
        let picked: Vec<PathBuf> = if folder {
            app.dialog().file().set_title("Pilih folder untuk diamankan").blocking_pick_folder()
                .and_then(|p| p.into_path().ok()).into_iter().collect()
        } else {
            app.dialog().file().set_title("Pilih file untuk diamankan").blocking_pick_files()
                .unwrap_or_default().into_iter().filter_map(|p| p.into_path().ok()).collect()
        };
        if picked.is_empty() { return Ok(None); }
        add_blocking(&app, picked, move_original).map(Some)
    }).await.map_err(|e| e.to_string())?
}

/// Isi file untuk pratinjau di aplikasi (gambar/teks), dikirim langsung sebagai biner.
#[tauri::command]
async fn read_file(app: AppHandle, id: String) -> R<tauri::ipc::Response> {
    let (blobs, meta) = blobs_and_meta(&app, &id)?;
    if meta.size > PREVIEW_LIMIT { return Err("File terlalu besar untuk pratinjau. Gunakan Buka.".into()); }
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let mut out = Vec::with_capacity(meta.size as usize);
        store::decrypt_to(&blobs, &meta, &mut out, |_, _| {}).map(|_| out)
    }).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Buka dengan aplikasi bawaan sistem lewat salinan sementara yang dihapus saat dikunci.
#[tauri::command]
async fn open_external(app: AppHandle, id: String) -> R<()> {
    let (blobs, meta) = blobs_and_meta(&app, &id)?;
    let view = st(&app).view.lock().unwrap().join(uuid::Uuid::new_v4().to_string());
    let app2 = app.clone();
    let path = tauri::async_runtime::spawn_blocking(move || -> R<PathBuf> {
        std::fs::create_dir_all(&view).map_err(|e| e.to_string())?;
        let dest = view.join(&meta.name);
        let label = format!("Membuka {}", meta.name);
        store::decrypt_to_path(&blobs, &meta, &dest, |done, total| {
            let _ = app2.emit("progress", Progress { label: label.clone(), file: 1, files: 1, done, total });
        }).map_err(|e| e.to_string())?;
        let _ = app2.emit("progress-done", ());
        Ok(dest)
    }).await.map_err(|e| e.to_string())??;
    open_with_os(&path)
}

/// Keluarkan satu file (tidak terenkripsi) ke lokasi pilihan.
#[tauri::command]
async fn export_one(app: AppHandle, id: String) -> R<bool> {
    let (blobs, meta) = blobs_and_meta(&app, &id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(dest) = app.dialog().file().set_file_name(&meta.name).blocking_save_file() else { return Ok(false) };
        let dest = dest.into_path().map_err(|e| e.to_string())?;
        let label = format!("Mengeluarkan {}", meta.name);
        store::decrypt_to_path(&blobs, &meta, &dest, |done, total| {
            let _ = app.emit("progress", Progress { label: label.clone(), file: 1, files: 1, done, total });
        }).map_err(|e| e.to_string())?;
        let _ = app.emit("progress-done", ());
        Ok(true)
    }).await.map_err(|e| e.to_string())?
}

/// Keluarkan semua file ke satu folder (untuk pindah komputer atau berhenti memakai).
#[tauri::command]
async fn export_all(app: AppHandle) -> R<Option<usize>> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(dir) = app.dialog().file().set_title("Pilih folder tujuan").blocking_pick_folder() else { return Ok(None) };
        let dir = dir.into_path().map_err(|e| e.to_string())?;
        let (blobs, metas) = {
            let s = st(&app); let g = s.store.lock().unwrap();
            let store = g.as_ref().ok_or("Terkunci.")?;
            (store.blobs_dir(), store.index.files.clone())
        };
        let n = metas.len();
        for (i, m) in metas.iter().enumerate() {
            let mut dest = dir.join(&m.name);
            let mut k = 2;
            while dest.exists() {
                let p = Path::new(&m.name);
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
                dest = dir.join(match p.extension().and_then(|e| e.to_str()) { Some(e) => format!("{stem} ({k}).{e}"), None => format!("{stem} ({k})") });
                k += 1;
            }
            let label = format!("Mengeluarkan {}", m.name);
            store::decrypt_to_path(&blobs, m, &dest, |done, total| {
                let _ = app.emit("progress", Progress { label: label.clone(), file: i + 1, files: n, done, total });
            }).map_err(|e| e.to_string())?;
        }
        let _ = app.emit("progress-done", ());
        Ok(Some(n))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn rename(app: AppHandle, id: String, name: String) -> R<()> {
    let s = st(&app); let mut g = s.store.lock().unwrap();
    g.as_mut().ok_or("Terkunci.")?.rename(&id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete(app: AppHandle, id: String) -> R<()> {
    let s = st(&app); let mut g = s.store.lock().unwrap();
    g.as_mut().ok_or("Terkunci.")?.delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn change_master(app: AppHandle, old: String, new: String) -> R<()> {
    let (old, new) = (Zeroizing::new(old), Zeroizing::new(new));
    tauri::async_runtime::spawn_blocking(move || {
        let dir = st(&app).dir.lock().unwrap().clone();
        Store::open(&dir, &old).map_err(|_| "Password lama salah.".to_string())?;
        let s = st(&app); let mut g = s.store.lock().unwrap();
        g.as_mut().ok_or("Terkunci.")?.change_master(&new, KdfCost::default()).map_err(|e| e.to_string())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn reveal(app: AppHandle) -> R<()> {
    let dir = st(&app).dir.lock().unwrap().clone();
    open_with_os(&dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let data = app.path().app_data_dir().expect("folder data");
            let cache = app.path().app_cache_dir().expect("folder cache");
            let s = app.state::<AppState>();
            *s.dir.lock().unwrap() = data.join("lemari");
            *s.view.lock().unwrap() = cache.join("view");
            let _ = std::fs::remove_dir_all(cache.join("view")); // sisa sesi sebelumnya
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            status, create, unlock, lock, list, add_paths, add_dialog, read_file, open_external,
            export_one, export_all, rename, delete, change_master, reveal
        ])
        .build(tauri::generate_context!())
        .expect("gagal memulai")
        .run(|app, ev| {
            if let tauri::RunEvent::Exit = ev { wipe_view(app); }
        });
}
