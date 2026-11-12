import React, { useState } from 'react';
import { DiskTelemetry } from '../types';
import {
  HardDrive,
  ShieldCheck,
  Trash2,
  Layers,
  RefreshCw,
  Plus,
  KeyRound,
  Lock,
  Unlock,
  Copy,
  X,
  AlertTriangle,
} from 'lucide-react';

interface DisksPageProps {
  disks: DiskTelemetry[];
  onMountToggle: (diskId: string, currentStatus: string) => void;
  onSync: (diskId: string) => void;
  onDelete: (diskId: string, deleteRemoteData: boolean) => void;
  onVerify: (diskId: string) => void;
  onExportKey: (diskId: string) => Promise<string | null>;
  onImportKey: (diskId: string, recoveryKey: string) => Promise<boolean>;
  onOpenCreateModal?: () => void;
}

function formatBytes(bytes: number, dec = 1): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const s = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dec))} ${s[i]}`;
}

export const DisksPage: React.FC<DisksPageProps> = ({
  disks,
  onMountToggle,
  onSync,
  onDelete,
  onVerify,
  onExportKey,
  onImportKey,
  onOpenCreateModal,
}) => {
  const [selectedId, setSelectedId] = useState<string | null>(disks[0]?.disk_id ?? null);
  const selectedDisk = disks.find((d) => d.disk_id === selectedId) ?? disks[0];
  const [isSyncingId, setIsSyncingId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<DiskTelemetry | null>(null);
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [keyBusy, setKeyBusy] = useState(false);
  const [importKeyValue, setImportKeyValue] = useState('');
  const [copyStatus, setCopyStatus] = useState<string | null>(null);

  const selectDisk = (diskId: string) => {
    setSelectedId(diskId);
    setRecoveryKey(null);
    setImportKeyValue('');
    setCopyStatus(null);
  };

  const handleSyncAction = (id: string) => {
    setIsSyncingId(id);
    onSync(id);
    setTimeout(() => setIsSyncingId(null), 1500);
  };

  const handleExportKey = async (diskId: string) => {
    setKeyBusy(true);
    setCopyStatus(null);
    try {
      const key = await onExportKey(diskId);
      if (key) setRecoveryKey(key);
    } finally {
      setKeyBusy(false);
    }
  };

  const handleImportKey = async (diskId: string) => {
    const value = importKeyValue.trim();
    if (value.replace(/^vssd:/i, '').replace(/\s/g, '').length !== 64) {
      setCopyStatus('Recovery key must contain exactly 64 hexadecimal characters.');
      return;
    }
    setKeyBusy(true);
    setCopyStatus(null);
    try {
      const imported = await onImportKey(diskId, value);
      if (imported) {
        setImportKeyValue('');
        setCopyStatus('Recovery key imported.');
      }
    } finally {
      setKeyBusy(false);
    }
  };

  const handleCopyKey = async () => {
    if (!recoveryKey) return;
    try {
      await navigator.clipboard.writeText(recoveryKey);
      setCopyStatus('Recovery key copied to the clipboard.');
    } catch {
      setCopyStatus('Clipboard access was denied. Select the key text and copy it manually.');
    }
  };

  const remoteExists = (disk: DiskTelemetry) =>
    disk.vhdx_file_size > 0 || disk.vhdx_chunks > 0 || disk.committed_chunks > 0;

  return (
    <div className="w-full flex flex-col gap-6 select-none animate-fade-in pb-8">
      {/* ── Headline Title & Subtitle ─────────────────────────────── */}
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div className="flex flex-col">
          <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
            Disks &amp; Volumes
          </h1>
          <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
            Manage VHDX volumes, unmounted snapshot mirroring, encryption keys, and Windows drive mount points
          </p>
        </div>
        {onOpenCreateModal && (
          <button
            id="disks-create-disk-button"
            onClick={onOpenCreateModal}
            className="bg-[#18191d] hover:bg-black text-white font-bold text-xs px-4 py-2.5 rounded-full flex items-center gap-2 shadow-xs transition-all"
          >
            <Plus className="w-3.5 h-3.5 text-[#d4f938] stroke-[3]" />
            <span>Create New Disk</span>
          </button>
        )}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 items-start">
        {/* ── Left Disk List Column ── */}
        <div className="lg:col-span-5 flex flex-col gap-3">
          {disks.map((disk) => {
            const isOnline = disk.status === 'Online';
            const isSelected = disk.disk_id === selectedDisk?.disk_id;
            const allocPct = disk.total_chunks > 0
              ? Math.min(100, Math.round((disk.allocated_chunks / disk.total_chunks) * 100))
              : 0;

            return (
              <div
                key={disk.disk_id}
                onClick={() => selectDisk(disk.disk_id)}
                className={`p-5 rounded-[24px] border transition-all cursor-pointer flex flex-col gap-3 ${
                  isSelected
                    ? 'bg-white border-[#18191d] shadow-md ring-2 ring-black/5'
                    : 'bg-white border-black/[0.04] shadow-xs hover:border-black/10'
                }`}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <div className={`w-10 h-10 rounded-2xl flex items-center justify-center ${
                      isSelected ? 'bg-[#18191d] text-white' : 'bg-[#f4f5f8] text-[#18191d]'
                    }`}>
                      <HardDrive className="w-5 h-5 stroke-[2]" />
                    </div>
                    <div>
                      <h3 className="font-bold text-sm text-[#18191d] leading-tight flex items-center gap-1.5">
                        {disk.name}
                        {disk.encryption_enabled && (
                          disk.encryption_key_present
                            ? <Lock className="w-3 h-3 text-emerald-600" aria-label="Encrypted" />
                            : <AlertTriangle className="w-3 h-3 text-amber-500" aria-label="Recovery key missing" />
                        )}
                      </h3>
                      <span className="text-[11px] font-medium text-zinc-400">
                        {disk.provider_name} {disk.drive_letter ? `· Drive ${disk.drive_letter}` : ''}
                      </span>
                    </div>
                  </div>

                  <span className={`px-2.5 py-0.5 text-[11px] font-extrabold rounded-full ${
                    isOnline
                      ? 'bg-[#d4f938] text-black'
                      : 'bg-zinc-100 text-zinc-600'
                  }`}>
                    {disk.status}
                  </span>
                </div>

                {/* Progress bar */}
                <div className="flex flex-col gap-1.5 pt-1">
                  <div className="flex justify-between text-[11px] font-semibold text-zinc-500">
                    <span>{formatBytes(disk.capacity_bytes)}</span>
                    <span className="text-zinc-800">{allocPct}% Allocated</span>
                  </div>
                  <div className="w-full bg-zinc-100 h-2 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-[#a29bfe] rounded-full transition-all duration-500"
                      style={{ width: `${allocPct}%` }}
                    />
                  </div>
                </div>
              </div>
            );
          })}

          {disks.length === 0 && (
            <div className="bg-white rounded-[24px] p-12 text-center text-zinc-400 border border-black/[0.04] flex flex-col items-center gap-2">
              <HardDrive className="w-8 h-8 text-zinc-300" />
              <p className="text-sm font-bold text-zinc-700">No Virtual Disks Found</p>
              <p className="text-xs">Create a new disk to start mounting Windows block devices.</p>
            </div>
          )}
        </div>

        {/* ── Right Column: Selected Disk Inspector ── */}
        {selectedDisk ? (
          <div className="lg:col-span-7 flex flex-col gap-5">
            {/* Main Action Card */}
            <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-6">
              <div className="flex items-start justify-between gap-4">
                <div className="flex items-center gap-3.5">
                  <div className="w-12 h-12 rounded-2xl bg-[#18191d] flex items-center justify-center text-white shadow-sm">
                    <HardDrive className="w-6 h-6 text-[#d4f938]" />
                  </div>
                  <div>
                    <h2 className="text-xl font-extrabold text-[#18191d] tracking-tight">
                      {selectedDisk.name}
                    </h2>
                    <p className="text-xs font-mono text-zinc-400 mt-0.5">
                      {selectedDisk.disk_id}
                    </p>
                  </div>
                </div>

                <div className="flex items-center gap-2 flex-wrap">
                  <button
                    onClick={() => handleSyncAction(selectedDisk.disk_id)}
                    disabled={isSyncingId === selectedDisk.disk_id || selectedDisk.status === 'Online'}
                    title={selectedDisk.status === 'Online' ? 'Unmount the disk before mirroring its VHDX snapshot' : 'Mirror the unmounted VHDX snapshot to the provider'}
                    className="bg-white border border-black/[0.08] hover:bg-zinc-50 text-zinc-800 font-semibold text-xs px-4 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <RefreshCw className={`w-3.5 h-3.5 ${isSyncingId === selectedDisk.disk_id ? 'animate-spin text-black' : 'text-zinc-600'}`} />
                    <span>Mirror Snapshot</span>
                  </button>

                  <button
                    onClick={() => onMountToggle(selectedDisk.disk_id, selectedDisk.status)}
                    disabled={selectedDisk.encryption_enabled && !selectedDisk.encryption_key_present}
                    title={
                      selectedDisk.encryption_enabled && !selectedDisk.encryption_key_present
                        ? 'Import the recovery key before mounting this encrypted disk'
                        : undefined
                    }
                    className={`font-semibold text-xs px-5 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed ${
                      selectedDisk.status === 'Online'
                        ? 'bg-rose-50 text-rose-700 hover:bg-rose-100 border border-rose-200'
                        : 'bg-[#18191d] text-white hover:bg-black'
                    }`}
                  >
                    {selectedDisk.status === 'Online' ? 'Dismount Device' : 'Mount to Windows (V:)'}
                  </button>
                </div>
              </div>

              {selectedDisk.encryption_enabled && !selectedDisk.encryption_key_present && (
                <div className="p-3.5 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 flex items-start gap-2.5">
                  <AlertTriangle className="w-4 h-4 text-amber-600 shrink-0 mt-0.5" />
                  <p className="text-[11px] font-medium leading-relaxed">
                    This disk is encrypted, but its DPAPI-protected key is not installed on this
                    machine. Import the 64-character recovery key below before mounting or syncing.
                  </p>
                </div>
              )}

              {/* Status Indicators Row */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2 border-t border-black/[0.04]">
                <div className="p-3 bg-[#f4f5f8] rounded-2xl">
                  <span className="text-[11px] font-semibold text-zinc-400 block">Status</span>
                  <span className="text-sm font-extrabold text-zinc-900 mt-0.5 block">
                    {selectedDisk.status}
                  </span>
                </div>
                <div className="p-3 bg-[#f4f5f8] rounded-2xl">
                  <span className="text-[11px] font-semibold text-zinc-400 block">Mount Point</span>
                  <span className="text-sm font-extrabold text-zinc-900 mt-0.5 block">
                    {selectedDisk.drive_letter ?? 'Not Mounted'}
                  </span>
                </div>
                <div className="p-3 bg-[#f4f5f8] rounded-2xl">
                  <span className="text-[11px] font-semibold text-zinc-400 block">VHDX Snapshot</span>
                  <span className="text-sm font-extrabold text-zinc-900 mt-0.5 block">
                    {selectedDisk.vhdx_file_size > 0 ? formatBytes(selectedDisk.vhdx_file_size) : 'Not mirrored'}
                  </span>
                </div>
                <div className="p-3 bg-[#f4f5f8] rounded-2xl">
                  <span className="text-[11px] font-semibold text-zinc-400 block">Encryption</span>
                  <span className="text-sm font-extrabold text-zinc-900 mt-0.5 block flex items-center gap-1">
                    {selectedDisk.encryption_enabled
                      ? <Lock className="w-3.5 h-3.5 text-emerald-600" />
                      : <Unlock className="w-3.5 h-3.5 text-zinc-400" />}
                    {selectedDisk.encryption_enabled ? 'AES-256-GCM' : 'Disabled'}
                  </span>
                </div>
              </div>
            </div>

            {/* Geometry & Device Specifications */}
            <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-4">
              <div className="flex items-center gap-2">
                <Layers className="w-4 h-4 text-zinc-500" />
                <h3 className="font-extrabold text-sm text-[#18191d]">
                  Device Geometry & Storage Properties
                </h3>
              </div>

              <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
                {[
                  {
                    label: 'Windows Device Path',
                    value: selectedDisk.physical_drive_path ?? 'Not attached',
                    mono: true,
                  },
                  { label: 'Chunk Geometry', value: '4 MiB (4,194,304 B)', mono: true },
                  { label: 'Sector Sizing', value: '512e / 4096 Physical', mono: true },
                  { label: 'Total Capacity', value: formatBytes(selectedDisk.capacity_bytes), mono: false },
                  { label: 'Local Cache', value: formatBytes(selectedDisk.cached_bytes), mono: false },
                  { label: 'Cloud Provider', value: selectedDisk.provider_name, mono: false },
                  { label: 'Total Chunks', value: selectedDisk.total_chunks.toLocaleString(), mono: false },
                  { label: 'Allocated Chunks', value: selectedDisk.allocated_chunks.toLocaleString(), mono: false },
                  {
                    label: 'Mirrored VHDX Chunks',
                    value: `${selectedDisk.mirrored_chunks.toLocaleString()} / ${selectedDisk.vhdx_chunks.toLocaleString()}`,
                    mono: false,
                  },
                  {
                    label: 'Snapshot Size',
                    value: selectedDisk.vhdx_file_size > 0 ? formatBytes(selectedDisk.vhdx_file_size) : 'Not mirrored',
                    mono: false,
                  },
                  {
                    label: 'Recovery Key',
                    value: selectedDisk.encryption_enabled
                      ? selectedDisk.encryption_key_present ? 'Present on this machine' : 'MISSING — import required'
                      : 'Not applicable',
                    mono: false,
                  },
                  { label: 'Integrity Checksum', value: 'BLAKE3-256 (on-demand audit)', mono: true },
                ].map((item) => (
                  <div key={item.label} className="p-3.5 bg-[#f4f5f8] rounded-2xl border border-black/[0.03]">
                    <span className="text-[11px] font-semibold text-zinc-400 block leading-tight">
                      {item.label}
                    </span>
                    <span className={`text-xs font-bold text-zinc-800 mt-1 block truncate ${item.mono ? 'font-mono' : ''}`}>
                      {item.value}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* Encryption Key Management */}
            {selectedDisk.encryption_enabled && (
              <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-4">
                <div className="flex items-center gap-2">
                  <KeyRound className="w-4 h-4 text-zinc-500" />
                  <h3 className="font-extrabold text-sm text-[#18191d]">Recovery Key</h3>
                </div>
                <p className="text-[11px] text-zinc-500 leading-relaxed">
                  The 64-character key is protected with Windows DPAPI and never uploaded. Export it
                  before moving this disk to another installation; without it, encrypted chunks cannot
                  be recovered.
                </p>

                <div className="flex flex-wrap items-center gap-2">
                  <button
                    onClick={() => handleExportKey(selectedDisk.disk_id)}
                    disabled={keyBusy || !selectedDisk.encryption_key_present}
                    className="bg-[#f4f5f8] hover:bg-zinc-200 text-zinc-800 font-bold text-xs px-4 py-2.5 rounded-full transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
                  >
                    <KeyRound className="w-3.5 h-3.5" />
                    <span>{keyBusy ? 'Working…' : 'Export Recovery Key'}</span>
                  </button>

                  <div className="flex items-center gap-2 flex-1 min-w-[260px]">
                    <input
                      type="text"
                      spellCheck={false}
                      placeholder="Paste 64-character recovery key"
                      value={importKeyValue}
                      onChange={(e) => setImportKeyValue(e.target.value)}
                      className="flex-1 bg-[#f4f5f8] border border-black/[0.05] rounded-full px-4 py-2.5 text-[11px] font-mono text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10"
                    />
                    <button
                      onClick={() => handleImportKey(selectedDisk.disk_id)}
                      disabled={keyBusy || importKeyValue.trim().length === 0}
                      className="bg-[#18191d] hover:bg-black text-white font-bold text-xs px-4 py-2.5 rounded-full transition-all disabled:opacity-40 disabled:cursor-not-allowed"
                    >
                      Import Key
                    </button>
                  </div>
                </div>

                {recoveryKey && (
                  <div className="p-3.5 rounded-2xl bg-zinc-900 text-zinc-100 flex flex-col gap-2">
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-[10px] font-bold uppercase tracking-wide text-zinc-400">
                        Recovery key — store it offline
                      </span>
                      <button
                        onClick={handleCopyKey}
                        className="text-zinc-300 hover:text-white flex items-center gap-1 text-[10px] font-bold"
                      >
                        <Copy className="w-3 h-3" /> Copy
                      </button>
                    </div>
                    <code className="text-[11px] font-mono break-all select-all">{recoveryKey}</code>
                  </div>
                )}

                {copyStatus && (
                  <p className="text-[11px] font-medium text-zinc-600">{copyStatus}</p>
                )}
              </div>
            )}

            {/* Production Maintenance Actions */}
            <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex items-center justify-between flex-wrap gap-4">
              <div>
                <h4 className="font-extrabold text-sm text-[#18191d]">Integrity & Maintenance</h4>
                <p className="text-xs text-zinc-500 mt-0.5">
                  Verify BLAKE3 hashes over the local VHDX snapshot chunks
                </p>
              </div>

              <div className="flex items-center gap-3">
                <button
                  onClick={() => onVerify(selectedDisk.disk_id)}
                  className="bg-[#d4f938] hover:bg-[#c6ec2d] text-black font-extrabold text-xs px-4 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2"
                >
                  <ShieldCheck className="w-4 h-4 stroke-[2.5]" />
                  <span>Verify Hashes</span>
                </button>

                <button
                  onClick={() => setDeleteTarget(selectedDisk)}
                  className="text-rose-500 hover:text-rose-700 hover:bg-rose-50 p-2.5 rounded-full transition-colors"
                  title="Delete Virtual Disk"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>
          </div>
        ) : null}
      </div>

      {/* Delete confirmation modal with an explicit remote-data choice. */}
      {deleteTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          <div className="absolute inset-0 bg-black/50 backdrop-blur-sm" onClick={() => setDeleteTarget(null)} />
          <div className="relative bg-white rounded-[28px] p-6 shadow-2xl border border-black/[0.04] max-w-lg w-full z-10 flex flex-col gap-4">
            <div className="flex items-start justify-between gap-4">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-2xl bg-rose-50 flex items-center justify-center">
                  <Trash2 className="w-5 h-5 text-rose-600" />
                </div>
                <div>
                  <h2 className="text-lg font-extrabold text-[#18191d]">Delete '{deleteTarget.name}'?</h2>
                  <p className="text-[11px] text-zinc-500 mt-0.5">
                    {remoteExists(deleteTarget)
                      ? `The provider holds ${deleteTarget.mirrored_chunks.toLocaleString()} mirrored VHDX chunk(s)` +
                        (deleteTarget.vhdx_file_size > 0 ? ` (${formatBytes(deleteTarget.vhdx_file_size)} snapshot)` : '') +
                        ' and a published manifest.'
                      : 'This disk has no recorded remote VHDX snapshot.'}
                  </p>
                </div>
              </div>
              <button
                onClick={() => setDeleteTarget(null)}
                className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center text-zinc-500 hover:text-black transition-colors"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="p-3.5 rounded-2xl bg-rose-50 border border-rose-200 text-rose-800 text-[11px] font-medium leading-relaxed">
              Deleting local data removes the VHDX, cache, WAL, manifest, and recovery key from this
              machine. Remote deletion is permanent and cannot be undone.
            </div>

            <div className="flex flex-col gap-2">
              {remoteExists(deleteTarget) && (
                <button
                  onClick={() => {
                    const target = deleteTarget;
                    setDeleteTarget(null);
                    onDelete(target.disk_id, true);
                  }}
                  className="w-full bg-rose-600 hover:bg-rose-700 text-white font-bold text-xs px-4 py-3 rounded-full transition-all"
                >
                  Delete local copy and remote snapshot
                </button>
              )}
              <button
                onClick={() => {
                  const target = deleteTarget;
                  setDeleteTarget(null);
                  onDelete(target.disk_id, false);
                }}
                className="w-full bg-[#18191d] hover:bg-black text-white font-bold text-xs px-4 py-3 rounded-full transition-all"
              >
                {remoteExists(deleteTarget) ? 'Delete local copy, keep remote snapshot' : 'Delete local copy'}
              </button>
              <button
                onClick={() => setDeleteTarget(null)}
                className="w-full text-zinc-600 hover:text-black font-semibold text-xs px-4 py-2 rounded-full transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
