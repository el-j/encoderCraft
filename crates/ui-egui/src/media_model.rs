//! Media Browser: location navigation, history, filtering and thumbnails for picking sources.
//!
//! Directory listings are read on a worker thread and capped so slow or huge folders never freeze the UI.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::SystemTime;

pub const MAX_ENTRIES: usize = 20_000;
pub const THUMB_CACHE_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    Video,
    Audio,
    Image,
    Other,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Folder => "Folder",
            EntryKind::Video => "Video",
            EntryKind::Audio => "Audio",
            EntryKind::Image => "Image",
            EntryKind::Other => "Other",
        }
    }

    /// Kinds the encoder can take as sources.
    pub fn is_media(self) -> bool {
        matches!(self, EntryKind::Video | EntryKind::Audio | EntryKind::Image)
    }
}

/// Classifies by extension only: probing every file in a folder would be slow and unsafe on hostile input.
pub fn kind_of(path: &Path, is_dir: bool) -> EntryKind {
    if is_dir {
        return EntryKind::Folder;
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mov" | "mkv" | "webm" | "mxf" | "avi" | "m4v" | "mpg" | "mpeg" | "ts" => EntryKind::Video,
        "mp3" | "wav" | "flac" | "aiff" | "aif" | "ogg" | "oga" | "m4a" | "aac" | "opus" => EntryKind::Audio,
        "png" | "jpg" | "jpeg" | "webp" | "tif" | "tiff" | "exr" | "bmp" | "gif" => EntryKind::Image,
        _ => EntryKind::Other,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Name,
    Kind,
    Size,
    Modified,
}

/// A fixed-capacity cache that evicts the least recently inserted entry.
pub struct BoundedCache<V> {
    capacity: usize,
    map: HashMap<PathBuf, V>,
    order: VecDeque<PathBuf>,
}

impl<V> BoundedCache<V> {
    pub fn new(capacity: usize) -> Self {
        Self { capacity: capacity.max(1), map: HashMap::new(), order: VecDeque::new() }
    }

    pub fn get(&self, key: &Path) -> Option<&V> {
        self.map.get(key)
    }

    pub fn contains(&self, key: &Path) -> bool {
        self.map.contains_key(key)
    }

    pub fn insert(&mut self, key: PathBuf, value: V) {
        if self.map.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.map.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else { break };
            self.map.remove(&oldest);
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

struct Listing {
    path: PathBuf,
    result: Result<(Vec<Entry>, bool), String>,
}

pub struct BrowserModel {
    location: PathBuf,
    history: Vec<PathBuf>,
    cursor: usize,
    entries: Vec<Entry>,
    truncated: bool,
    pub error: Option<String>,
    pub loading: bool,
    pub search: String,
    pub kind_filter: Option<EntryKind>,
    pub sort: SortBy,
    pub descending: bool,
    pub show_hidden: bool,
    pub favorites: Vec<PathBuf>,
    pending: Option<Receiver<Listing>>,
}

impl BrowserModel {
    pub fn new(start: PathBuf) -> Self {
        let mut m = Self {
            location: start.clone(),
            history: vec![start],
            cursor: 0,
            entries: Vec::new(),
            truncated: false,
            error: None,
            loading: false,
            search: String::new(),
            kind_filter: None,
            sort: SortBy::Name,
            descending: false,
            show_hidden: false,
            favorites: Vec::new(),
            pending: None,
        };
        m.refresh();
        m
    }

    pub fn location(&self) -> &Path {
        &self.location
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.history.len()
    }

    pub fn navigate(&mut self, path: PathBuf) {
        if path == self.location {
            return;
        }
        self.history.truncate(self.cursor + 1);
        self.history.push(path.clone());
        self.cursor = self.history.len() - 1;
        self.location = path;
        self.refresh();
    }

    pub fn back(&mut self) {
        if let Some(i) = self.cursor.checked_sub(1)
            && let Some(p) = self.history.get(i)
        {
            self.cursor = i;
            self.location = p.clone();
            self.refresh();
        }
    }

    pub fn forward(&mut self) {
        let i = self.cursor + 1;
        if let Some(p) = self.history.get(i) {
            self.cursor = i;
            self.location = p.clone();
            self.refresh();
        }
    }

    pub fn up(&mut self) {
        if let Some(parent) = self.location.parent().map(Path::to_path_buf) {
            self.navigate(parent);
        }
    }

    pub fn toggle_favorite(&mut self, path: &Path) {
        if let Some(i) = self.favorites.iter().position(|f| f == path) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(path.to_path_buf());
        }
    }

    /// Starts a background read of the current location.
    pub fn refresh(&mut self) {
        let (tx, rx) = channel();
        let path = self.location.clone();
        self.loading = true;
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let result = read_dir_bounded(&path);
            let _ = tx.send(Listing { path, result });
        });
    }

    /// Applies a finished background listing. Returns true when the visible content changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.pending else { return false };
        match rx.try_recv() {
            Ok(listing) => {
                self.pending = None;
                self.loading = false;
                if listing.path != self.location {
                    // Navigated away while this listing was loading; the newer request will answer.
                    return false;
                }
                match listing.result {
                    Ok((entries, truncated)) => {
                        self.entries = entries;
                        self.truncated = truncated;
                        self.error = None;
                    }
                    Err(e) => {
                        self.entries.clear();
                        self.truncated = false;
                        self.error = Some(e);
                    }
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.loading = false;
                self.error = Some("Folder listing was interrupted".into());
                true
            }
        }
    }

    /// Entries after search, type filter and sorting. Folders always come first.
    pub fn visible(&self) -> Vec<&Entry> {
        let q = self.search.trim().to_lowercase();
        let mut out: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .filter(|e| e.kind == EntryKind::Folder || self.kind_filter.is_none_or(|k| e.kind == k))
            .filter(|e| q.is_empty() || e.name.to_lowercase().contains(&q))
            .collect();
        out.sort_by(|a, b| {
            let folders = (b.kind == EntryKind::Folder).cmp(&(a.kind == EntryKind::Folder));
            let by = match self.sort {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Kind => a.kind.label().cmp(b.kind.label()).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Modified => a.modified.cmp(&b.modified),
            };
            folders.then(if self.descending { by.reverse() } else { by })
        });
        out
    }
}

fn read_dir_bounded(path: &Path) -> Result<(Vec<Entry>, bool), String> {
    let rd = std::fs::read_dir(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!("Permission denied: {}", path.display()),
        std::io::ErrorKind::NotFound => format!("Folder not found (is the volume connected?): {}", path.display()),
        _ => format!("Cannot read {}: {e}", path.display()),
    })?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for item in rd.flatten() {
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        let p = item.path();
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        entries.push(Entry {
            kind: kind_of(&p, meta.is_dir()),
            name,
            path: p,
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified: meta.modified().ok(),
        });
    }
    Ok((entries, truncated))
}

/// Well-known places and mounted volumes for the sidebar, only listing what exists.
pub fn standard_locations() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if let Some(h) = home {
        for (label, sub) in [
            ("Home", ""),
            ("Desktop", "Desktop"),
            ("Documents", "Documents"),
            ("Movies", "Movies"),
            ("Videos", "Videos"),
            ("Music", "Music"),
            ("Pictures", "Pictures"),
        ] {
            let p = if sub.is_empty() { h.clone() } else { h.join(sub) };
            if p.is_dir() {
                out.push((label.to_string(), p));
            }
        }
    }
    if cfg!(windows) {
        for letter in 'C'..='Z' {
            let root = PathBuf::from(format!("{letter}:\\"));
            if root.is_dir() {
                out.push((format!("Drive {letter}:"), root));
            }
        }
    } else {
        for base in ["/Volumes", "/media", "/mnt"] {
            if let Ok(rd) = std::fs::read_dir(base) {
                for e in rd.flatten().take(64) {
                    if e.path().is_dir() {
                        out.push((e.file_name().to_string_lossy().into_owned(), e.path()));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait(m: &mut BrowserModel) {
        let end = Instant::now() + Duration::from_secs(10);
        while m.loading && Instant::now() < end {
            m.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!m.loading, "listing did not finish");
    }

    fn sample() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("b.mov"), vec![0; 20]).unwrap();
        std::fs::write(dir.path().join("a.wav"), vec![0; 10]).unwrap();
        std::fs::write(dir.path().join("c.png"), vec![0; 30]).unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.path().join(".hidden.mp4"), b"x").unwrap();
        dir
    }

    #[test]
    fn lists_sorted_with_folders_first_and_hides_dotfiles() {
        let dir = sample();
        let mut m = BrowserModel::new(dir.path().to_path_buf());
        wait(&mut m);
        let names: Vec<_> = m.visible().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["sub", "a.wav", "b.mov", "c.png", "notes.txt"]);
        m.show_hidden = true;
        assert_eq!(m.visible().len(), 6);
    }

    #[test]
    fn filters_search_and_sorting() {
        let dir = sample();
        let mut m = BrowserModel::new(dir.path().to_path_buf());
        wait(&mut m);
        m.kind_filter = Some(EntryKind::Video);
        let names: Vec<_> = m.visible().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["sub", "b.mov"], "folders stay visible under a type filter");
        m.kind_filter = None;
        m.search = "C.P".into();
        assert_eq!(m.visible().iter().filter(|e| e.kind != EntryKind::Folder).count(), 1);
        m.search.clear();
        m.sort = SortBy::Size;
        m.descending = true;
        let files: Vec<_> = m.visible().iter().filter(|e| e.kind != EntryKind::Folder).map(|e| e.name.clone()).collect();
        assert_eq!(files, ["c.png", "b.mov", "a.wav", "notes.txt"]);
    }

    #[test]
    fn history_back_forward_up() {
        let dir = sample();
        let sub = dir.path().join("sub");
        let mut m = BrowserModel::new(dir.path().to_path_buf());
        assert!(!m.can_back());
        m.navigate(sub.clone());
        wait(&mut m);
        assert!(m.can_back() && !m.can_forward());
        m.back();
        assert_eq!(m.location(), dir.path());
        assert!(m.can_forward());
        m.forward();
        assert_eq!(m.location(), sub);
        m.up();
        assert_eq!(m.location(), dir.path());
        m.back();
        m.navigate(dir.path().join("elsewhere"));
        assert!(!m.can_forward(), "new navigation drops the forward history");
    }

    #[test]
    fn missing_folder_reports_an_error_not_a_crash() {
        let dir = sample();
        let mut m = BrowserModel::new(dir.path().join("gone"));
        wait(&mut m);
        assert!(m.error.as_deref().is_some_and(|e| e.contains("not found")));
        assert!(m.visible().is_empty());
    }

    #[test]
    fn favorites_toggle() {
        let dir = sample();
        let mut m = BrowserModel::new(dir.path().to_path_buf());
        m.toggle_favorite(dir.path());
        assert_eq!(m.favorites.len(), 1);
        m.toggle_favorite(dir.path());
        assert!(m.favorites.is_empty());
    }

    #[test]
    fn cache_evicts_oldest_entries() {
        let mut c = BoundedCache::new(2);
        c.insert("a".into(), 1);
        c.insert("b".into(), 2);
        c.insert("c".into(), 3);
        assert_eq!(c.len(), 2);
        assert!(!c.contains(Path::new("a")) && c.contains(Path::new("c")));
        c.insert("b".into(), 9);
        assert_eq!(c.get(Path::new("b")), Some(&9));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn classification_is_by_extension() {
        assert_eq!(kind_of(Path::new("x.MOV"), false), EntryKind::Video);
        assert_eq!(kind_of(Path::new("x.flac"), false), EntryKind::Audio);
        assert_eq!(kind_of(Path::new("x.exr"), false), EntryKind::Image);
        assert_eq!(kind_of(Path::new("x"), false), EntryKind::Other);
        assert_eq!(kind_of(Path::new("x.mov"), true), EntryKind::Folder);
    }
}
