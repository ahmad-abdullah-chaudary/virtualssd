export type DiskStatus =
  | 'Connecting'
  | 'Online'
  | 'Degraded'
  | 'Offline'
  | 'Recovering'
  | 'ReadOnly'
  | 'Unmounted'
  | 'NeedsAttention';

export interface DiskTelemetry {
  disk_id: string;
  name: string;
  status: DiskStatus;
  capacity_bytes: number;
  total_chunks: number;
  allocated_chunks: number;
  committed_chunks: number;
  cached_bytes: number;
  dirty_bytes: number;
  pending_sync_count: number;
  drive_letter: string | null;
  physical_drive_path: string | null;
  provider_name: string;
  last_error: string | null;
  /** Size of the VHDX at the last successful snapshot sync; 0 = never mirrored. */
  vhdx_file_size: number;
  /** Number of VHDX file chunks recorded in the manifest. */
  vhdx_chunks: number;
  /** VHDX chunks whose remote object id is recorded. */
  mirrored_chunks: number;
  /** True when chunk payloads are encrypted with the disk key before upload. */
  encryption_enabled: boolean;
  /** Whether the DPAPI-protected key for this encrypted disk is present. */
  encryption_key_present: boolean;
}

export interface AppSettings {
  cache_limit_gb: number;
  encrypt_new_disks: boolean;
  telemetry_poll_interval_ms: number;
  verify_after_sync: boolean;
  default_capacity_gb: number;
}

/** Runtime facts reported by the backend about the running binary. */
export interface RuntimeStatus {
  elevated: boolean;
  /** The exact version baked into the shipped binary, e.g. `1.0.0+42`. */
  version: string;
  /** The numeric build number that changes with every build. */
  build: string;
  company: string;
  developer: string;
}

export interface SettingsSnapshot {
  settings: AppSettings;
  /** Set when the stored settings file could not be read or was invalid. */
  warning: string | null;
}

/** Result of mirroring the local VHDX snapshot to the bound provider. */
export interface MirrorReport {
  scanned_chunks: number;
  uploaded_chunks: number;
  deleted_chunks: number;
  skipped_chunks: number;
  file_size_bytes: number;
  commit_version: number;
  warnings: string[];
}

export interface SyncDiskReport {
  mirror: MirrorReport;
  integrity: IntegrityReport | null;
}

export interface ImportReport {
  disk: DiskTelemetry;
  has_vhdx_snapshot: boolean;
  vhdx_file_size: number;
  vhdx_chunks: number;
  requires_recovery_key: boolean;
  warnings: string[];
}

export interface DeleteReport {
  remote_data_requested: boolean;
  remote: { deleted_chunks: number; deleted_manifest: boolean } | null;
}

/** The result of creating a disk, including anything the provider warned about. */
export interface CreateDiskReport {
  disk: DiskTelemetry;
  /** Non-fatal problems, such as a manifest index that could not be pinned. */
  warnings: string[];
}

export interface AppStats {
  total_capacity_bytes: number;
  total_allocated_bytes: number;
  total_cached_bytes: number;
  total_dirty_bytes: number;
  active_disks_count: number;
  mounted_disks_count: number;
}

/** Result of a local-cache BLAKE3 audit returned by the Tauri backend. */
export interface IntegrityReport {
  disk_id: string;
  disk_name: string;
  verified_chunks: number;
  missing_chunks: number;
  total_chunks: number;
  errors: string[];
  passed: boolean;
  disk_error: string | null;
}

/**
 * A single real backend log entry captured by the Rust `tracing` subscriber
 * and streamed to the UI. The frontend never synthesizes these.
 */
export interface EngineLogEntry {
  /** Monotonic per-process id, used to de-duplicate the initial fetch against live events. */
  id: number;
  /** Local wall-clock time (`HH:MM:SS`) shown in the console column. */
  ts: string;
  /** RFC 3339 timestamp of the backend event. */
  timestamp: string;
  level: 'INFO' | 'WARN' | 'ERROR' | 'DEBUG';
  /** Rust target the event came from, e.g. `storage-core::crypto`. */
  module: string;
  msg: string;
}

export interface TransferJob {
  id: string;
  diskName: string;
  chunkIndex: number;
  type: 'upload' | 'download' | 'verify';
  progress: number;
  speedMbps: number;
  status: 'active' | 'queued' | 'completed' | 'retrying' | 'failed';
  attempt: number;
}

/** Matches Rust ProviderRecord returned by the Tauri backend */
export interface ProviderRecord {
  id: string;
  kind: 'GoogleDrive' | 'Telegram';
  display_name: string;
  connected: boolean;
  error_message: string | null;
  total_quota_bytes: number;
  used_quota_bytes: number;
}

/** Legacy type kept for compatibility */
export interface ProviderAccount {
  id: string;
  type: 'GoogleDrive' | 'Telegram';
  accountName: string;
  status: 'Connected' | 'Expired' | 'RateLimited' | 'Disconnected';
  totalQuota: number;
  usedQuota: number;
  activeDisks: number;
}

