import React, { useState, useEffect, useCallback } from 'react';
import { Header } from './components/Header';
import { Sidebar, NavTab } from './components/Sidebar';
import { DashboardPage } from './pages/DashboardPage';
import { DisksPage } from './pages/DisksPage';
import { CreateDiskModal } from './pages/CreateDiskModal';
import { ProvidersPage } from './pages/ProvidersPage';
import { CachePage } from './pages/CachePage';
import { TransferQueuePage } from './pages/TransferQueuePage';
import { IntegrityPage } from './pages/IntegrityPage';
import { RecoveryPage } from './pages/RecoveryPage';
import { DiagnosticsPage } from './pages/DiagnosticsPage';
import { SettingsPage } from './pages/SettingsPage';
import { DiskTelemetry, AppStats, ProviderRecord, IntegrityReport, AppSettings, SettingsSnapshot, SyncDiskReport, ImportReport, DeleteReport, EngineLogEntry, RuntimeStatus } from './types';

// ── Tauri IPC bridge with stateful storage engine ───────────────────────────
const STORAGE_KEY = 'virtualssd_active_disks';

const DEFAULT_SETTINGS: AppSettings = {
  cache_limit_gb: 50,
  encrypt_new_disks: false,
  telemetry_poll_interval_ms: 10_000,
  verify_after_sync: false,
  default_capacity_gb: 100,
};

function formatBytesShort(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / 1024 ** index).toFixed(1)} ${units[index]}`;
}

function getLocalDisks(): DiskTelemetry[] {
  if (typeof window === 'undefined') return [];
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored) {
    try {
      const parsed = JSON.parse(stored) as Array<Partial<DiskTelemetry>>;
      // Older preview entries predate the snapshot/encryption fields.
      return parsed.map((disk) => ({
        disk_id: disk.disk_id ?? '',
        name: disk.name ?? 'Unknown',
        status: disk.status ?? 'Unmounted',
        capacity_bytes: disk.capacity_bytes ?? 0,
        total_chunks: disk.total_chunks ?? 0,
        allocated_chunks: disk.allocated_chunks ?? 0,
        committed_chunks: disk.committed_chunks ?? 0,
        cached_bytes: disk.cached_bytes ?? 0,
        dirty_bytes: disk.dirty_bytes ?? 0,
        pending_sync_count: disk.pending_sync_count ?? 0,
        drive_letter: disk.drive_letter ?? null,
        physical_drive_path: disk.physical_drive_path ?? null,
        provider_name: disk.provider_name ?? 'Browser preview',
        last_error: disk.last_error ?? null,
        vhdx_file_size: disk.vhdx_file_size ?? 0,
        vhdx_chunks: disk.vhdx_chunks ?? 0,
        mirrored_chunks: disk.mirrored_chunks ?? 0,
        encryption_enabled: disk.encryption_enabled ?? false,
        encryption_key_present: disk.encryption_key_present ?? false,
      }));
    } catch {
      // ignore parse error
    }
  }
  return [];
}

function saveLocalDisks(disks: DiskTelemetry[]) {
  if (typeof window !== 'undefined') {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(disks));
  }
}

async function callTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<T>(cmd, args);
  }

  // ── Browser preview fallback bridge ────────────────────────────────────────
  const PROVIDERS_KEY = 'virtualssd_providers';
  const getLocalProviders = (): ProviderRecord[] => {
    try { return JSON.parse(localStorage.getItem(PROVIDERS_KEY) || '[]'); } catch { return []; }
  };
  const saveLocalProviders = (p: ProviderRecord[]) => localStorage.setItem(PROVIDERS_KEY, JSON.stringify(p));

  if (cmd === 'get_runtime_status') {
    // The browser preview is not the shipped binary, so it must not claim a
    // release version. It reports a clearly-labelled preview marker instead.
    return {
      elevated: false,
      version: 'preview (not the desktop binary)',
      build: '0',
      company: 'Chaudary Global',
      developer: 'Ahmad Abdullah Chaudary',
    } as unknown as T;
  }

  const SETTINGS_KEY = 'virtualssd_settings';
  const defaultSettings: AppSettings = DEFAULT_SETTINGS;
  const readPreviewSettings = (): AppSettings => {
    try {
      return { ...defaultSettings, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) || '{}') };
    } catch {
      return defaultSettings;
    }
  };

  if (cmd === 'get_settings') {
    return { settings: readPreviewSettings(), warning: null } as unknown as T;
  }

  if (cmd === 'update_settings') {
    const { settings } = args as { settings: AppSettings };
    if (settings.cache_limit_gb < 1 || settings.cache_limit_gb > 4096) {
      throw new Error('Cache limit must be between 1 and 4096 GiB');
    }
    if (settings.telemetry_poll_interval_ms < 500 || settings.telemetry_poll_interval_ms > 60_000) {
      throw new Error('Telemetry interval must be between 500 and 60000 ms');
    }
    if (settings.default_capacity_gb < 1 || settings.default_capacity_gb > 65_536) {
      throw new Error('Default capacity must be between 1 and 65536 GB');
    }
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    return { settings, warning: null } as unknown as T;
  }

  if (cmd === 'restart_elevated') {
    throw new Error('Browser preview cannot relaunch as administrator. Run the desktop application.');
  }

  if (cmd === 'import_disk') {
    throw new Error('Browser preview cannot download a remote manifest. Run the desktop application.');
  }

  if (cmd === 'export_disk_key' || cmd === 'import_disk_key') {
    throw new Error('Browser preview has no real DPAPI encryption key. Run the desktop application.');
  }

  if (cmd === 'list_providers') {
    return getLocalProviders() as unknown as T;
  }

  if (cmd === 'add_google_drive_provider') {
    const { clientId, refreshToken, displayName } = args as { clientId: string; refreshToken: string; displayName: string };
    const record: ProviderRecord = {
      id: crypto.randomUUID(),
      kind: 'GoogleDrive',
      display_name: displayName,
      connected: false,
      error_message: 'Browser preview mode: real OAuth not performed. Run tauri dev to test real backend.',
      total_quota_bytes: 0,
      used_quota_bytes: 0,
    };
    const existing = getLocalProviders();
    saveLocalProviders([...existing, record]);
    return record as unknown as T;
  }

  if (cmd === 'add_telegram_provider') {
    const { botToken, storageChatId, displayName } = args as { botToken: string; storageChatId: number; displayName: string };
    const record: ProviderRecord = {
      id: crypto.randomUUID(),
      kind: 'Telegram',
      display_name: displayName,
      connected: false,
      error_message: 'Browser preview mode: real Bot API not called. Run tauri dev to test real backend.',
      total_quota_bytes: 0,
      used_quota_bytes: 0,
    };
    const existing = getLocalProviders();
    saveLocalProviders([...existing, record]);
    return record as unknown as T;
  }

  if (cmd === 'verify_provider') {
    const { providerId } = args as { providerId: string };
    const providers = getLocalProviders();
    const found = providers.find(p => p.id === providerId);
    if (!found) throw new Error('Provider not found');
    return { ...found, error_message: 'Browser preview: real verification requires Tauri runtime.' } as unknown as T;
  }

  if (cmd === 'remove_provider') {
    const { providerId } = args as { providerId: string };
    saveLocalProviders(getLocalProviders().filter(p => p.id !== providerId));
    return null as unknown as T;
  }

  if (cmd === 'list_disks') {
    return getLocalDisks() as unknown as T;
  }

  if (cmd === 'get_app_stats') {
    const disks = getLocalDisks();
    const totalCap = disks.reduce((a, d) => a + d.capacity_bytes, 0);
    const totalAlloc = disks.reduce((a, d) => a + d.allocated_chunks * 4 * 1024 * 1024, 0);
    const totalCached = disks.reduce((a, d) => a + d.cached_bytes, 0);
    const totalDirty = disks.reduce((a, d) => a + d.dirty_bytes, 0);
    const mounted = disks.filter((d) => d.status === 'Online').length;
    return {
      total_capacity_bytes: totalCap,
      total_allocated_bytes: totalAlloc,
      total_cached_bytes: totalCached,
      total_dirty_bytes: totalDirty,
      active_disks_count: disks.length,
      mounted_disks_count: mounted,
    } as unknown as T;
  }

  if (cmd === 'create_disk') {
    const { name, providerAccountId, capacityGb } = args as {
      name: string;
      providerAccountId: string;
      capacityGb: number;
    };
    const providers = getLocalProviders();
    const provider = providers.find(p => p.id === providerAccountId);
    if (!provider) {
      throw new Error(`Provider '${providerAccountId}' not found. Add a provider first.`);
    }
    const capBytes = capacityGb * 1024 * 1024 * 1024;
    const totalChunks = Math.floor(capBytes / (4 * 1024 * 1024));
    const newDisk: DiskTelemetry = {
      disk_id: crypto.randomUUID(),
      name,
      status: 'Unmounted',
      capacity_bytes: capBytes,
      total_chunks: totalChunks,
      allocated_chunks: 0,
      committed_chunks: 0,
      cached_bytes: 0,
      dirty_bytes: 0,
      pending_sync_count: 0,
      drive_letter: null,
      physical_drive_path: null,
      provider_name: provider.kind === 'Telegram' ? 'Telegram Bot API' : 'Google Drive (OAuth2)',
      last_error: 'Browser preview: disk created locally, not synced to real backend.',
      vhdx_file_size: 0,
      vhdx_chunks: 0,
      mirrored_chunks: 0,
      encryption_enabled: readPreviewSettings().encrypt_new_disks,
      encryption_key_present: false,
    };
    const updated = [...getLocalDisks(), newDisk];
    saveLocalDisks(updated);
    return newDisk as unknown as T;
  }

  if (cmd === 'mount_disk') {
    throw new Error(
      'Browser preview cannot attach a Windows block device. Run the desktop application as administrator to mount a VHDX.'
    );
  }

  if (cmd === 'unmount_disk') {
    const { diskId } = args as { diskId: string };
    const existing = getLocalDisks();
    const updated = existing.map((d) =>
      d.disk_id === diskId ? { ...d, status: 'Unmounted' as const, drive_letter: null } : d
    );
    saveLocalDisks(updated);
    const found = updated.find((d) => d.disk_id === diskId);
    return found as unknown as T;
  }

  if (cmd === 'sync_disk') {
    throw new Error(
      'Browser preview cannot upload a VHDX snapshot. Run the desktop application with a connected provider.'
    );
  }

  if (cmd === 'sync_disk_live') {
    throw new Error(
      'Browser preview cannot create a volume shadow copy. Run the desktop application as administrator.'
    );
  }

  if (cmd === 'delete_disk') {
    const { diskId } = args as { diskId: string };
    saveLocalDisks(getLocalDisks().filter((d) => d.disk_id !== diskId));
    return { remote_data_requested: false, remote: null } as unknown as T;
  }

  if (cmd === 'verify_disk_integrity') {
    throw new Error(
      'Browser preview cannot audit a VHDX snapshot. Run the desktop application to verify real files.'
    );
  }

  if (cmd === 'get_engine_logs') {
    throw new Error(
      'Browser preview has no backend engine to read logs from. Run the desktop application.'
    );
  }

  if (cmd === 'clear_engine_logs') {
    throw new Error(
      'Browser preview has no backend engine log buffer to clear. Run the desktop application.'
    );
  }

  throw new Error(`Unhandled command: ${cmd}`);
}

// ── Root component ────────────────────────────────────────────────────────────
export const App: React.FC = () => {
  const [activeTab, setActiveTab]           = useState<NavTab>('dashboard');
  const [isCreateModalOpen, setIsCreateModalOpen] = useState(false);
  const [isSyncing, setIsSyncing]           = useState(false);
  const [disks, setDisks]                   = useState<DiskTelemetry[]>([]);
  const [stats, setStats]                   = useState<AppStats | null>(null);
  const [providers, setProviders]           = useState<ProviderRecord[]>([]);
  const [elevated, setElevated]             = useState<boolean | null>(null);
  const [runtime, setRuntime]               = useState<RuntimeStatus | null>(null);
  const [settings, setSettings]             = useState<AppSettings>(DEFAULT_SETTINGS);
  const [settingsWarning, setSettingsWarning] = useState<string | null>(null);
  const [error, setError]                   = useState<string | null>(null);
  const [isLoading, setIsLoading]           = useState(true);

  // ── Data loading ────────────────────────────────────────────────────────────
  const loadData = useCallback(async () => {
    try {
      const [tauriDisks, tauriStats, tauriProviders, runtime, storedSettings] = await Promise.all([
        callTauri<DiskTelemetry[]>('list_disks'),
        callTauri<AppStats>('get_app_stats'),
        callTauri<ProviderRecord[]>('list_providers'),
        callTauri<RuntimeStatus>('get_runtime_status'),
        callTauri<SettingsSnapshot>('get_settings'),
      ]);
      setDisks(tauriDisks);
      setStats(tauriStats);
      setProviders(tauriProviders);
      setElevated(runtime.elevated);
      setRuntime(runtime);
      setSettings(storedSettings.settings);
      setSettingsWarning(storedSettings.warning);
      setError(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (!msg.includes('preview mode')) {
        setError(msg);
      }
      console.info('[VirtualSSD] Telemetry loaded via fallback bridge');
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Initial load plus polling at the interval chosen in Settings.
  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, settings.telemetry_poll_interval_ms);
    return () => clearInterval(interval);
  }, [loadData, settings.telemetry_poll_interval_ms]);

  // ── Real backend engine log stream ─────────────────────────────────────────
  //
  // These entries come from the Rust `tracing` subscriber via the
  // `get_engine_logs` command and live `engine-log` events. Nothing here is
  // synthesized in the browser.
  const [engineLogs, setEngineLogs] = useState<EngineLogEntry[]>([]);
  const [engineLogsAvailable, setEngineLogsAvailable] = useState(false);
  const [engineLogError, setEngineLogError] = useState<string | null>(null);

  const refreshEngineLogs = useCallback(async () => {
    try {
      const entries = await callTauri<EngineLogEntry[]>('get_engine_logs');
      setEngineLogs(entries);
      setEngineLogsAvailable(true);
      setEngineLogError(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setEngineLogsAvailable(false);
      setEngineLogError(msg);
    }
  }, []);

  // Load the existing backend buffer, then subscribe to live events. The
  // subscription is registered before the fetch so no entry can slip through
  // the gap between the two.
  useEffect(() => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
      setEngineLogsAvailable(false);
      setEngineLogError(
        'The live engine log stream requires the desktop backend. The browser preview has no Rust engine to read from.'
      );
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | null = null;

    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const stop = await listen<EngineLogEntry>('engine-log', (event) => {
          if (disposed) return;
          setEngineLogs((prev) => {
            if (prev.some((entry) => entry.id === event.payload.id)) return prev;
            return [event.payload, ...prev].slice(0, 500);
          });
        });
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setEngineLogError(`Could not subscribe to the backend log stream: ${msg}`);
      }

      await refreshEngineLogs();
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshEngineLogs]);

  const clearEngineLogs = useCallback(async () => {
    try {
      const entries = await callTauri<EngineLogEntry[]>('clear_engine_logs');
      setEngineLogs(entries);
      setEngineLogError(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setEngineLogError(`Could not clear the backend log buffer: ${msg}`);
    }
  }, []);

  // ── Disk actions ────────────────────────────────────────────────────────────
  const handleMountToggle = async (diskId: string, currentStatus: string) => {
    try {
      if (currentStatus === 'Online') {
        await callTauri('unmount_disk', { diskId });
        setNotification({
          text: `Disk ${diskId.slice(0, 8)}… was detached from the Windows Virtual Disk API.`,
          type: 'success',
        });
      } else {
        await callTauri('mount_disk', { diskId, driveLetter: 'V:' });
        setNotification({
          text: `Disk ${diskId.slice(0, 8)}… was attached and formatted as a Windows volume.`,
          type: 'success',
        });
      }
      setTimeout(() => setNotification(null), 6000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(
        msg.toLowerCase().includes('administrator')
          ? `${msg} Use "Restart as administrator" on the Settings page.`
          : `Mount/unmount failed: ${msg}`
      );
    }
  };

  const handleSyncLive = async (diskId: string) => {
    setIsSyncing(true);
    try {
      const report = await callTauri<SyncDiskReport>('sync_disk_live', { diskId });
      const mirror = report.mirror;
      const summary =
        `Mirrored the mounted disk (${formatBytesShort(mirror.file_size_bytes)}) in ` +
        `${mirror.uploaded_chunks} uploaded / ${mirror.deleted_chunks} deleted / ` +
        `${mirror.skipped_chunks} unchanged chunks`;

      setNotification({
        text: `${summary}. The volume stayed mounted and usable throughout.`,
        type: mirror.warnings.length > 0 ? 'info' : 'success',
      });
      setTimeout(() => setNotification(null), 8000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(
        msg.toLowerCase().includes('administrator')
          ? `${msg} Use "Restart as administrator" on the Settings page.`
          : `Live mirror failed: ${msg}`
      );
    } finally {
      setIsSyncing(false);
    }
  };

  const handleSync = async (diskId: string) => {
    setIsSyncing(true);
    try {
      const report = await callTauri<SyncDiskReport>('sync_disk', { diskId });
      const mirror = report.mirror;
      const summary =
        `Mirrored ${formatBytesShort(mirror.file_size_bytes)} in ` +
        `${mirror.uploaded_chunks} uploaded / ${mirror.deleted_chunks} deleted / ` +
        `${mirror.skipped_chunks} unchanged chunks`;

      if (report.integrity) {
        const audit = report.integrity;
        if (!audit.passed) {
          setNotification({
            text: `Snapshot synced, but the post-sync audit found ${audit.errors.length} checksum error(s).`,
            type: 'error',
          });
        } else {
          setNotification({
            text: `${summary}. Post-sync audit: ${audit.verified_chunks}/${audit.total_chunks} chunks verified.`,
            type: mirror.warnings.length > 0 ? 'info' : 'success',
          });
        }
      } else {
        setNotification({
          text: `${summary}.${mirror.warnings.length > 0 ? ` ${mirror.warnings.length} warning(s) were logged.` : ''}`,
          type: mirror.warnings.length > 0 ? 'info' : 'success',
        });
      }
      setTimeout(() => setNotification(null), 7000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(`Sync failed: ${msg}`);
    } finally {
      setIsSyncing(false);
    }
  };

  const handleDelete = async (diskId: string, deleteRemoteData: boolean) => {
    try {
      const report = await callTauri<DeleteReport>('delete_disk', { diskId, deleteRemoteData });
      setNotification({
        text: report.remote
          ? `Deleted disk locally and removed ${report.remote.deleted_chunks} remote chunk(s).`
          : 'Deleted disk locally. Remote data was left untouched.',
        type: 'success',
      });
      setTimeout(() => setNotification(null), 5000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(`Delete failed: ${msg}`);
    }
  };

  const handleImportDisk = async (providerId: string, diskId: string): Promise<boolean> => {
    try {
      const report = await callTauri<ImportReport>('import_disk', { providerId, diskId });
      setNotification({
        text:
          `Imported '${report.disk.name}'. ` +
          (report.has_vhdx_snapshot
            ? 'The VHDX will be restored from the provider on first mount.'
            : 'No snapshot exists yet; mounting will create a blank VHDX.') +
          (report.requires_recovery_key ? ' Import the recovery key before mounting.' : ''),
        type: report.requires_recovery_key ? 'info' : 'success',
      });
      setTimeout(() => setNotification(null), 8000);
      await loadData();
      return true;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNotification({ text: `Import failed: ${msg}`, type: 'error' });
      setTimeout(() => setNotification(null), 8000);
      return false;
    }
  };

  const handleUpdateSettings = async (next: AppSettings): Promise<SettingsSnapshot> => {
    const snapshot = await callTauri<SettingsSnapshot>('update_settings', { settings: next });
    setSettings(snapshot.settings);
    setSettingsWarning(snapshot.warning);
    return snapshot;
  };

  const handleExportKey = async (diskId: string): Promise<string | null> => {
    try {
      const key = await callTauri<string>('export_disk_key', { diskId });
      return key;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNotification({ text: `Recovery key export failed: ${msg}`, type: 'error' });
      setTimeout(() => setNotification(null), 7000);
      return null;
    }
  };

  const handleImportKey = async (diskId: string, recoveryKey: string): Promise<boolean> => {
    try {
      await callTauri('import_disk_key', { diskId, recoveryKey });
      setNotification({ text: 'Recovery key imported. The encrypted disk can now be mounted and synced.', type: 'success' });
      setTimeout(() => setNotification(null), 6000);
      await loadData();
      return true;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNotification({ text: `Recovery key import failed: ${msg}`, type: 'error' });
      setTimeout(() => setNotification(null), 7000);
      return false;
    }
  };

  const handleRestartElevated = async () => {
    try {
      await callTauri('restart_elevated');
      setNotification({ text: 'Starting an elevated instance… this window will close.', type: 'info' });
      setTimeout(() => setNotification(null), 4000);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNotification({ text: `Elevated relaunch failed: ${msg}`, type: 'error' });
      setTimeout(() => setNotification(null), 7000);
    }
  };

  const [notification, setNotification]   = useState<{ text: string; type: 'success' | 'error' | 'info' } | null>(null);

  const handleVerify = async (diskId: string): Promise<IntegrityReport | null> => {
    try {
      const report = await callTauri<IntegrityReport>('verify_disk_integrity', { diskId });
      const nothingAudited = report.total_chunks === 0 || (report.verified_chunks === 0 && report.total_chunks > 0);
      const summary = report.errors.length > 0
        ? `BLAKE3 audit found ${report.errors.length} checksum/cache error(s): ${report.errors
            .slice(0, 3)
            .join('; ')}${report.errors.length > 3 ? '…' : ''}`
        : report.total_chunks === 0
          ? 'No mirrored VHDX chunks or allocated cache chunks were available to audit.'
          : nothingAudited
            ? `No local chunks were available to audit (${report.missing_chunks} of ${report.total_chunks} not present locally). Integrity of those chunks is unverified.`
            : `Local BLAKE3 audit: ${report.verified_chunks} of ${report.total_chunks} chunks verified` +
              (report.missing_chunks > 0
                ? `; ${report.missing_chunks} not present locally and therefore not audited.`
                : '; no checksum failures.');

      setNotification({
        text: summary,
        type: report.errors.length > 0 ? 'error' : nothingAudited ? 'info' : 'success',
      });
      setTimeout(() => setNotification(null), 6000);
      return report;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setNotification({ text: `Integrity check error: ${msg}`, type: 'error' });
      return null;
    }
  };

  // ── Provider actions ──────────────────────────────────────────────────────
  const handleAddGoogleDriveProvider = async (clientId: string, refreshToken: string, displayName: string) => {
    try {
      const record = await callTauri<ProviderRecord>('add_google_drive_provider', { clientId, refreshToken, displayName });
      setNotification({
        text: record.connected
          ? `Google Drive '${record.display_name}' connected successfully.`
          : `Google Drive added but offline: ${record.error_message}`,
        type: record.connected ? 'success' : 'error',
      });
      setTimeout(() => setNotification(null), 5000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(`Failed to add Google Drive: ${msg}`);
      setNotification({
        text: `Google Drive addition failed: ${msg}`,
        type: 'error',
      });
      setTimeout(() => setNotification(null), 5000);
    }
  };

  const handleAddTelegramProvider = async (botToken: string, storageChatId: number, displayName: string) => {
    try {
      const record = await callTauri<ProviderRecord>('add_telegram_provider', { botToken, storageChatId, displayName });
      setNotification({
        text: record.connected
          ? `Telegram '${record.display_name}' connected successfully.`
          : `Telegram added but offline: ${record.error_message}`,
        type: record.connected ? 'success' : 'error',
      });
      setTimeout(() => setNotification(null), 5000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(`Failed to add Telegram: ${msg}`);
      setNotification({
        text: `Telegram addition failed: ${msg}`,
        type: 'error',
      });
      setTimeout(() => setNotification(null), 5000);
    }
  };

  const handleVerifyProvider = async (providerId: string) => {
    try {
      const record = await callTauri<ProviderRecord>('verify_provider', { providerId });
      setNotification({
        text: record.connected
          ? `Provider '${record.display_name}' verified as connected.`
          : `Provider '${record.display_name}' is not reachable: ${record.error_message}`,
        type: record.connected ? 'success' : 'error',
      });
      setTimeout(() => setNotification(null), 6000);
      await loadData();
      return record;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(`Provider verification failed: ${msg}`);
      throw e;
    }
  };

  const handleRemoveProvider = async (providerId: string) => {
    try {
      await callTauri('remove_provider', { providerId });
      setNotification({
        text: 'The provider and its encrypted credential were removed from this installation.',
        type: 'success',
      });
      setTimeout(() => setNotification(null), 6000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
    }
  };

  const handleCreateDisk = async (
    name: string,
    providerAccountId: string,
    capacityGb: number,
  ) => {
    try {
      await callTauri<DiskTelemetry>('create_disk', { name, providerAccountId, capacityGb });
      const provName = providers.find(p => p.id === providerAccountId)?.display_name ?? providerAccountId;
      setNotification({
        text: `Volume '${name}' was created on ${provName} and its manifest was published.`,
        type: 'success',
      });
      setTimeout(() => setNotification(null), 6000);
      await loadData();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(`Disk creation failed: ${msg}`);
    }
  };

  const totalPendingSync = disks.reduce((acc, d) => acc + d.pending_sync_count, 0);

  // The system tray can ask the window to switch to the Diagnostics view.
  useEffect(() => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | null = null;

    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const stop = await listen('show-diagnostics', () => {
          if (disposed) return;
          setActiveTab('diagnostics');
        });
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        console.warn(`[VirtualSSD] Could not subscribe to tray events: ${msg}`);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <div className="h-screen w-full bg-[#18191d] flex overflow-hidden font-sans">
      {/* Left Dark Sidebar */}
      <Sidebar
        activeTab={activeTab}
        setActiveTab={setActiveTab}
        pendingSyncCount={totalPendingSync}
        activeDisksCount={disks.length}
        version={runtime?.version ?? null}
      />

      {/* Right Light Content Canvas */}
      <div className="flex-1 bg-[#f2f4f7] rounded-l-[28px] p-6 lg:p-8 flex flex-col overflow-hidden">
          {/* Top Header */}
          <Header
            onOpenCreateModal={() => setIsCreateModalOpen(true)}
            onRefresh={() => {
              setIsSyncing(true);
              loadData().finally(() => setIsSyncing(false));
            }}
            isSyncing={isSyncing}
            activeDisksCount={disks.length}
            elevated={elevated}
            onRestartElevated={handleRestartElevated}
          />

          {/* Global notification banner */}
          {notification && (
            <div className={`my-2 px-4 py-2.5 rounded-2xl text-xs flex items-center justify-between shadow-xs transition-all ${
              notification.type === 'success'
                ? 'bg-emerald-50 border border-emerald-200 text-emerald-800'
                : notification.type === 'info'
                  ? 'bg-sky-50 border border-sky-200 text-sky-800'
                  : 'bg-rose-50 border border-rose-200 text-rose-800'
            }`}>
              <span className="font-semibold">{notification.text}</span>
              <button
                onClick={() => setNotification(null)}
                className="text-zinc-500 hover:text-black font-bold text-sm leading-none ml-2"
              >
                ×
              </button>
            </div>
          )}

          {/* Settings load warning is shown until the user saves settings. */}
          {settingsWarning && (
            <div className="my-2 px-4 py-2 bg-amber-50 border border-amber-200 rounded-xl text-xs text-amber-800 flex items-center justify-between">
              <span className="font-medium">Settings: {settingsWarning}</span>
              <button
                onClick={() => setSettingsWarning(null)}
                className="text-amber-600 hover:text-amber-800 font-bold text-sm leading-none ml-2"
              >
                ×
              </button>
            </div>
          )}

          {/* Global error banner if present */}
          {error && (
            <div className="my-2 px-4 py-2 bg-rose-50 border border-rose-200 rounded-xl text-xs text-rose-700 flex items-center justify-between">
              <span className="font-medium">{error}</span>
              <button
                onClick={() => setError(null)}
                className="text-rose-500 hover:text-rose-700 font-bold text-sm leading-none ml-2"
              >
                ×
              </button>
            </div>
          )}

          {/* Main Scrollable Canvas Area */}
          <main id="main-content" className="flex-1 overflow-y-auto mt-2 pr-1 scrollbar-none">
            {isLoading ? (
              <div className="flex items-center justify-center h-full">
                <div className="text-center space-y-3">
                  <div className="w-9 h-9 border-2 border-zinc-300 border-t-zinc-900 rounded-full animate-spin mx-auto" />
                  <p className="text-xs font-semibold text-zinc-500">Loading system overview…</p>
                </div>
              </div>
            ) : (
              <div className="w-full pb-6">
                {activeTab === 'dashboard' && (
                  <DashboardPage
                    disks={disks}
                    stats={stats}
                    onMountToggle={handleMountToggle}
                    onSync={handleSync}
                    onOpenCreateModal={() => setIsCreateModalOpen(true)}
                  />
                )}
                {activeTab === 'disks' && (
                  <DisksPage
                    disks={disks}
                    onMountToggle={handleMountToggle}
                    onSync={handleSync}
                    onSyncLive={handleSyncLive}
                    onDelete={handleDelete}
                    onVerify={handleVerify}
                    onExportKey={handleExportKey}
                    onImportKey={handleImportKey}
                    onOpenCreateModal={() => setIsCreateModalOpen(true)}
                    elevated={elevated}
                  />
                )}
                {activeTab === 'providers'   && <ProvidersPage
                  disks={disks}
                  providers={providers}
                  onAddGoogleDrive={handleAddGoogleDriveProvider}
                  onAddTelegram={handleAddTelegramProvider}
                  onVerify={handleVerifyProvider}
                  onRemove={handleRemoveProvider}
                  onImportDisk={handleImportDisk}
                />}
                {activeTab === 'cache'       && <CachePage disks={disks} stats={stats} />}
                {activeTab === 'transfers'   && <TransferQueuePage disks={disks} onSync={handleSync} onSyncLive={handleSyncLive} elevated={elevated} />}
                {activeTab === 'integrity'   && <IntegrityPage disks={disks} onVerify={handleVerify} />}
                {activeTab === 'journal'     && <RecoveryPage />}
                {activeTab === 'recovery'    && <RecoveryPage />}
                {activeTab === 'diagnostics' && (
                  <DiagnosticsPage
                    logs={engineLogs}
                    available={engineLogsAvailable}
                    error={engineLogError}
                    onRefresh={refreshEngineLogs}
                    onClear={clearEngineLogs}
                  />
                )}
                {activeTab === 'settings'    && (
                  <SettingsPage
                    settings={settings}
                    warning={settingsWarning}
                    elevated={elevated}
                    onSave={handleUpdateSettings}
                    onRestartElevated={handleRestartElevated}
                  />
                )}
              </div>
            )}
          </main>
        </div>

      <CreateDiskModal
        isOpen={isCreateModalOpen}
        onClose={() => setIsCreateModalOpen(false)}
        onCreate={handleCreateDisk}
        providers={providers}
        defaultCapacityGb={settings.default_capacity_gb}
        encryptNewDisks={settings.encrypt_new_disks}
      />
    </div>
  );
};
