use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadEntry {
    pub id: u64,
    pub url: String,
    pub filename: String,
    pub save_path: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub status: DownloadStatus,
    pub created_at: DateTime<Utc>,
    pub speed_bytes_per_sec: f64,
}

struct DownloadHandle {
    cancel: Arc<AtomicBool>,
}

pub struct DownloadManager {
    next_id: Arc<Mutex<u64>>,
    state: Arc<Mutex<Vec<DownloadEntry>>>,
    handles: Arc<Mutex<Vec<(u64, DownloadHandle)>>>,
}

impl DownloadManager {
    pub fn new() -> Self {
        Self {
            next_id: Arc::new(Mutex::new(1)),
            state: Arc::new(Mutex::new(Vec::new())),
            handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn save_state_to_settings(&self, settings: &fortrust_storage::SettingsDatabase) {
        let entries = self.state.lock().unwrap();
        let active: Vec<&DownloadEntry> = entries
            .iter()
            .filter(|e| matches!(e.status, DownloadStatus::Downloading | DownloadStatus::Paused | DownloadStatus::Queued))
            .collect();
        if active.is_empty() {
            return;
        }
        if let Ok(json) = serde_json::to_string(&active) {
            let _ = settings.store("chrome.downloads.state", &fortrust_storage::SettingValue::String(json));
        }
    }

    pub fn resume_all_paused(&mut self, save_dir: &str) {
        let ids: Vec<u64> = {
            let s = self.state.lock().unwrap();
            s.iter()
                .filter(|e| e.status == DownloadStatus::Paused)
                .map(|e| e.id)
                .collect()
        };
        for id in ids {
            self.resume_download(id, save_dir);
        }
    }

    pub fn load_state_from_settings(&mut self, settings: &fortrust_storage::SettingsDatabase) {
        let val = settings.load("chrome.downloads.state");
        let Some(val) = val else { return };
        let Some(json) = val.as_string() else { return };
        let Ok(entries): Result<Vec<DownloadEntry>, _> = serde_json::from_str(json) else { return };
        let mut resumed: Vec<DownloadEntry> = entries
            .into_iter()
            .map(|mut e| {
                e.status = DownloadStatus::Paused;
                e.speed_bytes_per_sec = 0.0;
                e
            })
            .collect();
        let max_id = resumed.iter().map(|e| e.id).max().unwrap_or(0);
        *self.next_id.lock().unwrap() = max_id + 1;
        let mut state = self.state.lock().unwrap();
        state.append(&mut resumed);
    }

    fn allocate_id(&mut self) -> u64 {
        let mut id = self.next_id.lock().unwrap();
        let i = *id;
        *id += 1;
        i
    }

    pub fn start_download(
        &mut self,
        url: String,
        filename: String,
        save_dir: &str,
        existing_id: Option<u64>,
    ) -> u64 {
        let id = existing_id.unwrap_or_else(|| self.allocate_id());
        let save_path = format!("{}\\{}", save_dir.trim_end_matches('\\'), filename);

        {
            let mut s = self.state.lock().unwrap();
            if existing_id.is_none() {
                s.push(DownloadEntry {
                    id,
                    url: url.clone(),
                    filename: filename.clone(),
                    save_path: save_path.clone(),
                    total_bytes: 0,
                    downloaded_bytes: 0,
                    status: DownloadStatus::Queued,
                    created_at: Utc::now(),
                    speed_bytes_per_sec: 0.0,
                });
            } else if let Some(existing) = s.iter_mut().find(|e| e.id == id) {
                existing.status = DownloadStatus::Queued;
                existing.speed_bytes_per_sec = 0.0;
            }
        }

        let state_ref = Arc::clone(&self.state);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel_flag);

        self.handles.lock().unwrap().push((id, DownloadHandle { cancel: cancel_flag }));

        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()
                .unwrap_or_else(|_| panic!("Failed to create reqwest client"));

            let downloaded = {
                let s = state_ref.lock().unwrap();
                s.iter()
                    .find(|e| e.id == id)
                    .map(|e| e.downloaded_bytes)
                    .unwrap_or(0)
            };

            let mut req = client.get(&url);
            if downloaded > 0 {
                req = req.header(reqwest::header::RANGE, format!("bytes={downloaded}-"));
            }

            match req.send() {
                Ok(response) => {
                    let resp_status = response.status().as_u16();
                    let total = if resp_status == 206 {
                        response
                            .content_length()
                            .map(|cl| downloaded + cl)
                            .unwrap_or(downloaded)
                    } else {
                        response.content_length().unwrap_or(0)
                    };

                    {
                        let mut s = state_ref.lock().unwrap();
                        if let Some(e) = s.iter_mut().find(|e| e.id == id) {
                            e.total_bytes = total;
                            e.status = DownloadStatus::Downloading;
                        }
                    }

                    if let Some(parent) = PathBuf::from(&save_path).parent() {
                        let _ = fs::create_dir_all(parent);
                    }

                    let file_result = if downloaded > 0 {
                        OpenOptions::new().append(true).create(true).open(&save_path)
                    } else {
                        File::create(&save_path)
                    };

                    match file_result {
                        Ok(mut file) => {
                            let mut dl = downloaded;
                            let mut last_update = Instant::now();
                            let mut last_bytes = dl;

                            let mut reader = response.take(1024 * 1024 * 1024); // 1GB limit
                            let mut buf = [0u8; 65536];
                            loop {
                                if cancel_clone.load(Ordering::SeqCst) {
                                    let mut s = state_ref.lock().unwrap();
                                    if let Some(e) = s.iter_mut().find(|en| en.id == id) {
                                        e.status = DownloadStatus::Paused;
                                        e.downloaded_bytes = dl;
                                    }
                                    return;
                                }

                                match reader.read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if file.write_all(&buf[..n]).is_err() {
                                            let mut s = state_ref.lock().unwrap();
                                            if let Some(en) = s.iter_mut().find(|en| en.id == id) {
                                                en.status = DownloadStatus::Failed("File write error".into());
                                            }
                                            return;
                                        }
                                        dl += n as u64;

                                        let now = Instant::now();
                                        let elapsed = now.duration_since(last_update);
                                        if elapsed >= Duration::from_millis(500) {
                                            let bytes_since = dl - last_bytes;
                                            let speed = bytes_since as f64 / elapsed.as_secs_f64();
                                            last_bytes = dl;
                                            last_update = now;
                                            let mut s = state_ref.lock().unwrap();
                                            if let Some(e) = s.iter_mut().find(|en| en.id == id) {
                                                e.downloaded_bytes = dl;
                                                e.speed_bytes_per_sec = speed;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Check if it was a timeout (might be recoverable)
                                        if e.kind() == ErrorKind::TimedOut
                                            || e.kind() == ErrorKind::ConnectionReset
                                            || e.kind() == ErrorKind::ConnectionAborted
                                        {
                                            // Save progress and pause for potential resume
                                            let mut s = state_ref.lock().unwrap();
                                            if let Some(en) = s.iter_mut().find(|en| en.id == id) {
                                                en.status = DownloadStatus::Paused;
                                                en.downloaded_bytes = dl;
                                            }
                                        } else {
                                            let mut s = state_ref.lock().unwrap();
                                            if let Some(en) = s.iter_mut().find(|en| en.id == id) {
                                                en.status = DownloadStatus::Failed(format!("Read error: {e}"));
                                                en.downloaded_bytes = dl;
                                            }
                                        }
                                        return;
                                    }
                                }
                            }

                            let mut s = state_ref.lock().unwrap();
                            if let Some(entry) = s.iter_mut().find(|en| en.id == id) {
                                entry.downloaded_bytes = dl;
                                if entry.status == DownloadStatus::Downloading {
                                    entry.status = DownloadStatus::Completed;
                                }
                            }
                        }
                        Err(e) => {
                            let mut s = state_ref.lock().unwrap();
                            if let Some(entry) = s.iter_mut().find(|en| en.id == id) {
                                entry.status = DownloadStatus::Failed(format!("Cannot open file: {e}"));
                            }
                        }
                    }
                }
                Err(e) => {
                    // Network errors are often transient — pause for potential resume
                    let mut s = state_ref.lock().unwrap();
                    if let Some(entry) = s.iter_mut().find(|en| en.id == id) {
                        entry.status = DownloadStatus::Failed(format!("HTTP error: {e}"));
                        entry.downloaded_bytes = downloaded;
                    }
                }
            }
        });

        id
    }

    pub fn pause_download(&mut self, id: u64) {
        let handles = self.handles.lock().unwrap();
        if let Some((_, handle)) = handles.iter().find(|(i, _)| *i == id) {
            handle.cancel.store(true, Ordering::SeqCst);
        }
        let mut s = self.state.lock().unwrap();
        if let Some(entry) = s.iter_mut().find(|e| e.id == id)
            && (entry.status == DownloadStatus::Downloading || entry.status == DownloadStatus::Queued)
        {
            entry.status = DownloadStatus::Paused;
        }
    }

    pub fn resume_download(&mut self, id: u64, save_dir: &str) {
        let entry = {
            let s = self.state.lock().unwrap();
            s.iter().find(|e| e.id == id).cloned()
        };
        if let Some(e) = entry
            && (e.status == DownloadStatus::Paused || matches!(e.status, DownloadStatus::Failed(_)))
        {
            // Clean up old handle (drop lock before calling start_download)
            {
                let mut handles = self.handles.lock().unwrap();
                handles.retain(|(i, _)| *i != id);
            }
            self.start_download(e.url, e.filename, save_dir, Some(id));
        }
    }

    pub fn remove_download(&mut self, id: u64) {
        let mut handles = self.handles.lock().unwrap();
        if let Some((_, handle)) = handles.iter().find(|(i, _)| *i == id) {
            handle.cancel.store(true, Ordering::SeqCst);
        }
        handles.retain(|(i, _)| *i != id);
        let mut s = self.state.lock().unwrap();
        s.retain(|e| e.id != id);
    }

    pub fn all_downloads(&self) -> Vec<DownloadEntry> {
        self.state.lock().unwrap().clone()
    }

    #[allow(dead_code)]
    pub fn entry_by_id(&self, id: u64) -> Option<DownloadEntry> {
        let s = self.state.lock().unwrap();
        s.iter().find(|e| e.id == id).cloned()
    }
}

pub fn default_download_dir() -> String {
    if cfg!(target_os = "windows") {
        std::env::var("USERPROFILE")
            .map(|p| format!("{p}\\Downloads"))
            .unwrap_or_else(|_| "C:\\Downloads".into())
    } else {
        std::env::var("HOME")
            .map(|p| format!("{p}/Downloads"))
            .unwrap_or_else(|_| "/tmp".into())
    }
}
