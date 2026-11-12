//! VirtualSSD Storage Service Scaffold.
//!
//! Enumerates existing disk directories at startup and then waits for Ctrl+C.
//! It does not expose a named-pipe or HTTP listener and is not registered as a
//! Windows service. The desktop application does not use this binary.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;
use virtualssd_block_map::DEFAULT_CHUNK_SIZE;
use virtualssd_manifest::{DiskManifest, ProviderType};
use virtualssd_storage_core::{DiskStatus, DiskTelemetry, VirtualDiskEngine};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    ListDisks,
    GetDiskTelemetry { disk_id: Uuid },
    CreateDisk {
        name: String,
        provider_type: ProviderType,
        account_id: String,
        capacity_bytes: u64,
    },
    MountDisk {
        disk_id: Uuid,
        preferred_letter: Option<char>,
    },
    UnmountDisk {
        disk_id: Uuid,
    },
    SyncDisk {
        disk_id: Uuid,
    },
    DeleteDisk {
        disk_id: Uuid,
        delete_remote_data: bool,
    },
    GetStorageStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Success,
    Disks(Vec<DiskTelemetry>),
    Telemetry(DiskTelemetry),
    Error(String),
}

pub struct StorageServiceState {
    data_dir: PathBuf,
    disks: RwLock<HashMap<Uuid, Arc<VirtualDiskEngine>>>,
}

impl StorageServiceState {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir).expect("Failed to create VirtualSSD data dir");
        let service = Self {
            data_dir,
            disks: RwLock::new(HashMap::new()),
        };
        service.discover_existing_disks();
        service
    }

    fn discover_existing_disks(&self) {
        let disks_dir = self.data_dir.join("disks");
        if let Ok(entries) = std::fs::read_dir(&disks_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_file = path.join("manifest.json");
                    if manifest_file.exists() {
                        if let Ok(manifest) = DiskManifest::load_with_fallback(&manifest_file) {
                            let id = manifest.disk_id;
                            if let Ok(engine) = VirtualDiskEngine::open(&path, manifest, None, 50 * 1024 * 1024 * 1024) {
                                info!("Discovered existing virtual disk: {} ({})", engine.telemetry().name, id);
                                self.disks.write().insert(id, Arc::new(engine));
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        match req {
            IpcRequest::ListDisks => {
                let disks = self.disks.read();
                let list: Vec<DiskTelemetry> = disks.values().map(|d| d.telemetry()).collect();
                IpcResponse::Disks(list)
            }
            IpcRequest::GetDiskTelemetry { disk_id } => {
                let disks = self.disks.read();
                if let Some(engine) = disks.get(&disk_id) {
                    IpcResponse::Telemetry(engine.telemetry())
                } else {
                    IpcResponse::Error(format!("Disk {disk_id} not found"))
                }
            }
            IpcRequest::CreateDisk {
                name,
                provider_type,
                account_id,
                capacity_bytes,
            } => {
                let manifest = match DiskManifest::new(
                    name.clone(),
                    provider_type,
                    account_id,
                    capacity_bytes,
                    DEFAULT_CHUNK_SIZE,
                ) {
                    Ok(m) => m,
                    Err(e) => return IpcResponse::Error(e.to_string()),
                };

                let disk_id = manifest.disk_id;
                let disk_dir = self.data_dir.join("disks").join(disk_id.to_string());

                match VirtualDiskEngine::open(&disk_dir, manifest, None, 50 * 1024 * 1024 * 1024) {
                    Ok(engine) => {
                        let arc_engine = Arc::new(engine);
                        self.disks.write().insert(disk_id, arc_engine);
                        info!("Successfully created VirtualSSD disk: {name} ({disk_id})");
                        IpcResponse::Success
                    }
                    Err(e) => IpcResponse::Error(e.to_string()),
                }
            }
            IpcRequest::MountDisk {
                disk_id,
                preferred_letter,
            } => {
                let disks = self.disks.read();
                if let Some(engine) = disks.get(&disk_id) {
                    engine.set_status(DiskStatus::Online);
                    let letter = preferred_letter.map(|c| c.to_string()).unwrap_or_else(|| "V:".to_string());
                    engine.set_drive_letter(Some(letter));
                    IpcResponse::Success
                } else {
                    IpcResponse::Error(format!("Disk {disk_id} not found"))
                }
            }
            IpcRequest::UnmountDisk { disk_id } => {
                let disks = self.disks.read();
                if let Some(engine) = disks.get(&disk_id) {
                    engine.set_status(DiskStatus::Unmounted);
                    engine.set_drive_letter(None);
                    IpcResponse::Success
                } else {
                    IpcResponse::Error(format!("Disk {disk_id} not found"))
                }
            }
            IpcRequest::SyncDisk { disk_id } => {
                let disks = self.disks.read();
                if let Some(engine) = disks.get(&disk_id) {
                    let engine_clone = engine.clone();
                    tokio::spawn(async move {
                        let _ = engine_clone.sync_dirty_chunks().await;
                    });
                    IpcResponse::Success
                } else {
                    IpcResponse::Error(format!("Disk {disk_id} not found"))
                }
            }
            IpcRequest::DeleteDisk {
                disk_id,
                delete_remote_data: _,
            } => {
                let mut disks = self.disks.write();
                if let Some(engine) = disks.remove(&disk_id) {
                    engine.set_status(DiskStatus::Unmounted);
                    let disk_dir = self.data_dir.join("disks").join(disk_id.to_string());
                    let _ = std::fs::remove_dir_all(disk_dir);
                    IpcResponse::Success
                } else {
                    IpcResponse::Error(format!("Disk {disk_id} not found"))
                }
            }
            IpcRequest::GetStorageStats => {
                let disks = self.disks.read();
                let list: Vec<DiskTelemetry> = disks.values().map(|d| d.telemetry()).collect();
                IpcResponse::Disks(list)
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting VirtualSSD Storage Service (Windows Service Daemon)...");

    let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    let base_dir = PathBuf::from(program_data).join("VirtualSSD");
    let _state = Arc::new(StorageServiceState::new(base_dir));

    info!("VirtualSSD Storage Service scaffold is running; no IPC listener is active.");

    // Keep running
    tokio::signal::ctrl_c().await?;
    info!("Shutting down VirtualSSD Storage Service safely...");

    Ok(())
}
