import React, { useState } from 'react';
import { AlertTriangle, CheckCircle2, Cloud, HardDrive, Lock, RefreshCw, Unlock } from 'lucide-react';
import { DiskTelemetry } from '../types';

interface TransferQueuePageProps {
  disks: DiskTelemetry[];
  onSync: (diskId: string) => void;
}

function fmtBytes(b: number) {
  if (b === 0) return '0 B';
  const k = 1024, s = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(b) / Math.log(k));
  return `${parseFloat((b / k ** i).toFixed(1))} ${s[i]}`;
}

export const TransferQueuePage: React.FC<TransferQueuePageProps> = ({ disks, onSync }) => {
  const [syncingId, setSyncingId] = useState<string | null>(null);

  const totalMirrored = disks.reduce((s, d) => s + d.mirrored_chunks, 0);
  const totalVhdxChunks = disks.reduce((s, d) => s + d.vhdx_chunks, 0);
  const totalSnapshot = disks.reduce((s, d) => s + d.vhdx_file_size, 0);
  const onlineDisks = disks.filter((d) => d.status === 'Online');

  const handleSync = (id: string) => {
    setSyncingId(id);
    onSync(id);
    setTimeout(() => setSyncingId(null), 1500);
  };

  return (
    <div className="w-full flex flex-col gap-5 select-none animate-fade-in pb-4">
      {/* ── Headline ──────────────────────────────────────────────── */}
      <div className="flex flex-col">
        <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
          Snapshot Mirror &amp; Sync
        </h1>
        <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
          Unmounted VHDX snapshot chunks, per-disk mirror status, and manual cloud publication
        </p>
      </div>

      {/* ── KPI Cards ─────────────────────────────────────────────── */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Mirrored VHDX Chunks</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Cloud className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {totalMirrored.toLocaleString()}
              <span className="text-xs font-bold text-zinc-500"> / {totalVhdxChunks.toLocaleString()}</span>
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              Chunks with a recorded remote object id
            </p>
          </div>
        </div>

        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Latest Snapshot Size</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <HardDrive className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {totalSnapshot > 0 ? fmtBytes(totalSnapshot) : 'None'}
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              {totalSnapshot > 0 ? 'Recorded in the published manifest' : 'No VHDX has been mirrored yet'}
            </p>
          </div>
        </div>

        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Mounted Volumes</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <CheckCircle2 className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {onlineDisks.length} / {disks.length}
            </span>
            <span className="bg-[#d4f938] text-black font-extrabold text-[10px] px-2 py-0.5 rounded-full">
              {disks.length === 0 ? 'No volumes' : onlineDisks.length > 0 ? 'Unmount to mirror' : 'Ready to mirror'}
            </span>
          </div>
        </div>
      </div>

      {/* ── Dark Pipeline Stats Bar ───────────────────────────────── */}
      <div className="bg-[#18191d] text-white rounded-[28px] p-6 shadow-md border border-white/[0.05] flex items-center justify-between gap-6 flex-wrap">
        <div className="flex flex-col gap-1 max-w-2xl">
          <span className="text-[11px] font-bold text-[#d4f938] uppercase tracking-wider">Mirroring Model</span>
          <h4 className="text-base font-extrabold tracking-tight">
            Manual, unmounted-only VHDX snapshot mirroring
          </h4>
          <p className="text-xs text-zinc-400 leading-relaxed">
            Changed non-zero 4 MiB chunks are uploaded, then the manifest is published last. All-zero
            regions are omitted. Engine cache dirty chunks are a separate namespace and are not
            uploaded by this snapshot sync. Throughput and ETA are not measured.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <div className="flex flex-col items-center">
            <div className="w-10 h-10 rounded-2xl bg-white/10 flex items-center justify-center mb-1">
              <Cloud className="w-5 h-5 text-[#d4f938]" />
            </div>
            <span className="text-[10px] text-zinc-400 font-semibold">G-Drive</span>
          </div>
          <div className="flex flex-col items-center">
            <div className="w-10 h-10 rounded-2xl bg-white/10 flex items-center justify-center mb-1">
              <Cloud className="w-5 h-5 text-[#a29bfe]" />
            </div>
            <span className="text-[10px] text-zinc-400 font-semibold">Telegram</span>
          </div>
        </div>
      </div>

      {/* ── Volume Snapshot Cards ─────────────────────────────────── */}
      <h2 className="text-lg font-extrabold tracking-tight text-[#18191d]">
        Volume Snapshot Status
      </h2>

      {disks.length === 0 ? (
        <div className="bg-white rounded-[24px] p-12 text-center text-zinc-400 border border-black/[0.04] flex flex-col items-center gap-2">
          <HardDrive className="w-8 h-8 text-zinc-300" />
          <p className="text-sm font-bold text-zinc-700">No Volumes</p>
          <p className="text-xs">Create a virtual disk to mirror its VHDX snapshot.</p>
        </div>
      ) : (
        disks.map((disk) => {
          const isOnline = disk.status === 'Online';
          const isSyncing = syncingId === disk.disk_id;
          const hasSnapshot = disk.vhdx_file_size > 0 || disk.vhdx_chunks > 0;
          const mirroredPct = disk.vhdx_chunks > 0
            ? Math.min(100, Math.round((disk.mirrored_chunks / disk.vhdx_chunks) * 100))
            : 0;

          return (
            <div key={disk.disk_id} className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-4">
              <div className="flex items-center justify-between gap-3 flex-wrap">
                <div className="flex items-center gap-3.5">
                  <div className="w-11 h-11 rounded-2xl bg-[#18191d] flex items-center justify-center text-white">
                    <HardDrive className="w-5 h-5 text-[#d4f938]" />
                  </div>
                  <div>
                    <h3 className="font-extrabold text-base text-[#18191d]">{disk.name}</h3>
                    <p className="text-[11px] font-mono text-zinc-400 mt-0.5">
                      {disk.disk_id} · {disk.provider_name}
                    </p>
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  <span className={`px-3 py-1 text-xs font-bold rounded-full ${
                    isOnline ? 'bg-amber-50 text-amber-800' : 'bg-zinc-100 text-zinc-700'
                  }`}>
                    {isOnline ? 'Mounted' : disk.status}
                  </span>
                  <button
                    onClick={() => handleSync(disk.disk_id)}
                    disabled={isSyncing || isOnline}
                    title={isOnline ? 'Unmount the disk before mirroring its VHDX snapshot' : 'Mirror the local VHDX snapshot'}
                    className="bg-white border border-black/[0.08] hover:bg-zinc-50 text-zinc-800 font-semibold text-xs px-4 py-2 rounded-full transition-all shadow-xs flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <RefreshCw className={`w-3.5 h-3.5 ${isSyncing ? 'animate-spin' : ''}`} />
                    {isSyncing ? 'Mirroring…' : isOnline ? 'Unmount to mirror' : 'Mirror Snapshot'}
                  </button>
                </div>
              </div>

              <div className="flex flex-col gap-2 p-4 bg-[#f4f5f8] rounded-2xl border border-black/[0.03]">
                <div className="flex justify-between text-xs font-semibold text-zinc-600 gap-3">
                  <span>Mirrored chunks vs VHDX chunks</span>
                  <span className="font-bold text-zinc-900">
                    {hasSnapshot
                      ? `${disk.mirrored_chunks.toLocaleString()} / ${disk.vhdx_chunks.toLocaleString()}`
                      : 'Not mirrored yet'}
                  </span>
                </div>
                <div className="w-full bg-zinc-200/80 h-2 rounded-full overflow-hidden">
                  <div
                    className="h-full bg-[#a29bfe] rounded-full transition-all duration-700"
                    style={{ width: `${mirroredPct}%` }}
                  />
                </div>
              </div>

              {isOnline && (
                <div className="p-3 rounded-xl bg-amber-50 border border-amber-200 text-amber-900 text-[11px] flex items-center gap-2">
                  <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                  <span>The disk is mounted. Unmount it before mirroring so the snapshot is consistent.</span>
                </div>
              )}

              {disk.encryption_enabled && !disk.encryption_key_present && (
                <div className="p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-900 text-[11px] flex items-center gap-2">
                  <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                  <span>The recovery key is missing. Import it before mirroring or restoring this encrypted disk.</span>
                </div>
              )}

              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                {[
                  { label: 'Snapshot Size', value: hasSnapshot ? fmtBytes(disk.vhdx_file_size) : 'Not mirrored' },
                  { label: 'VHDX Chunks', value: disk.vhdx_chunks.toLocaleString() },
                  { label: 'Mirrored Chunks', value: disk.mirrored_chunks.toLocaleString() },
                  {
                    label: 'Encryption',
                    value: disk.encryption_enabled
                      ? disk.encryption_key_present ? 'AES-256-GCM (key present)' : 'AES-256-GCM (key missing)'
                      : 'Disabled',
                  },
                ].map((item) => (
                  <div key={item.label} className="p-3 bg-[#f4f5f8] rounded-2xl border border-black/[0.03]">
                    <span className="text-[11px] font-semibold text-zinc-400 block">{item.label}</span>
                    <span className="font-bold text-zinc-900 text-xs mt-0.5 block flex items-center gap-1">
                      {item.label === 'Encryption' && (
                        disk.encryption_enabled
                          ? <Lock className="w-3 h-3" />
                          : <Unlock className="w-3 h-3 text-zinc-400" />
                      )}
                      {item.value}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          );
        })
      )}
    </div>
  );
};
