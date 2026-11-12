import React from 'react';
import { DiskTelemetry, AppStats } from '../types';
import {
  HardDrive,
  Cpu,
  ShieldCheck,
  Activity,
  CheckCircle2,
  AlertCircle,
  RefreshCw,
  Plus,
  Cloud,
} from 'lucide-react';

interface DashboardPageProps {
  disks: DiskTelemetry[];
  stats: AppStats | null;
  onMountToggle: (diskId: string, currentStatus: string) => void;
  onSync: (diskId: string) => void;
  onOpenCreateModal?: () => void;
}

function formatBytes(bytes: number, decimals = 1): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(decimals))} ${sizes[i]}`;
}

export const DashboardPage: React.FC<DashboardPageProps> = ({
  disks,
  stats,
  onMountToggle,
  onSync,
  onOpenCreateModal,
}) => {
  // Real aggregates calculated strictly from live disk telemetry
  const totalCap = stats?.total_capacity_bytes ?? disks.reduce((a, d) => a + d.capacity_bytes, 0);
  const totalAlloc = stats?.total_allocated_bytes ?? disks.reduce((a, d) => a + (d.allocated_chunks * 4 * 1024 * 1024), 0);
  const totalCached = stats?.total_cached_bytes ?? disks.reduce((a, d) => a + d.cached_bytes, 0);
  const totalDirty = stats?.total_dirty_bytes ?? disks.reduce((a, d) => a + d.dirty_bytes, 0);
  const totalAllocChunks = disks.reduce((a, d) => a + d.allocated_chunks, 0);
  const totalMirroredChunks = disks.reduce((a, d) => a + d.mirrored_chunks, 0);
  const totalVhdxChunks = disks.reduce((a, d) => a + d.vhdx_chunks, 0);
  const totalSnapshotBytes = disks.reduce((a, d) => a + d.vhdx_file_size, 0);

  const mountedDisks = disks.filter((d) => d.status === 'Online');
  const hasErrors = disks.some((d) => d.last_error !== null);

  const allocPct = totalCap > 0 ? Math.min(100, Math.round((totalAlloc / totalCap) * 100)) : 0;
  const cachePct = totalCap > 0 ? Math.min(100, Math.round((totalCached / totalCap) * 100)) : 0;
  const dirtyPct = totalCap > 0 ? Math.max(totalDirty > 0 ? 1 : 0, Math.min(100, Math.round((totalDirty / totalCap) * 100))) : 0;

  return (
    <div className="w-full flex flex-col gap-5 select-none animate-fade-in pb-4">
      {/* ── Headline Title & Real Active Metrics ──────────────────── */}
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div className="flex flex-col">
          <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
            Storage Overview
          </h1>
          <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
            Windows VHDX block device metrics, local cache status, and provider configuration
          </p>
        </div>

        {onOpenCreateModal && (
          <button
            onClick={onOpenCreateModal}
            className="bg-[#18191d] hover:bg-black text-white font-bold text-xs px-4 py-2.5 rounded-full flex items-center gap-2 shadow-xs transition-all"
          >
            <Plus className="w-3.5 h-3.5 text-[#d4f938] stroke-[3]" />
            <span>New Virtual Disk</span>
          </button>
        )}
      </div>

      {/* ── Main 5-Card Grid ──────────────────────────────────────── */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 items-start">
        {/* ── CARD 1: Left Tall White Card (Storage Allocation) ────── */}
        <div className="lg:col-span-5 bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col justify-between min-h-[460px] h-full">
          {/* Card Header */}
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="w-9 h-9 rounded-full bg-[#f4f5f8] border border-black/[0.04] flex items-center justify-center">
                <HardDrive className="w-4 h-4 text-[#18191d] stroke-[2.5]" />
              </div>
              <div>
                <h2 className="font-bold text-sm text-[#18191d]">Allocated Capacity</h2>
                <span className="text-[11px] text-zinc-400 font-medium">Virtual Block Pool</span>
              </div>
            </div>

            <span className="px-2.5 py-1 text-xs font-extrabold rounded-full bg-[#d4f938] text-black">
              {disks.length} Volume{disks.length === 1 ? '' : 's'}
            </span>
          </div>

          {/* Metric & Allocation Ratio */}
          <div className="mt-4 flex flex-col gap-1">
            <span className="text-4xl font-extrabold tracking-tight text-[#18191d]">
              {formatBytes(totalCap)}
            </span>
            <div className="flex items-center gap-2 text-xs font-semibold text-zinc-500">
              <span>{formatBytes(totalAlloc)} actively written</span>
              <span>·</span>
              <span className="text-zinc-800 font-bold">{allocPct}% utilized</span>
            </div>
          </div>

          {/* Storage Tier Breakdown Stack */}
          <div className="my-5 flex flex-col gap-3 p-4 bg-[#f8f9fb] rounded-2xl border border-black/[0.03]">
            <div className="flex items-center justify-between text-xs">
              <span className="font-bold text-zinc-800">Allocation &amp; Cache Indicators</span>
              <span className="font-mono text-[11px] text-zinc-500 font-semibold">{totalAllocChunks.toLocaleString()} chunks</span>
            </div>

            {/* Segmented capacity bar */}
            <div className="w-full bg-zinc-200 h-2.5 rounded-full overflow-hidden flex">
              <div
                style={{ width: `${Math.max(allocPct, 2)}%` }}
                className="bg-[#18191d] h-full transition-all duration-300"
                title={`Allocated Chunks: ${formatBytes(totalAlloc)}`}
              />
              <div
                style={{ width: `${cachePct}%` }}
                className="bg-[#a29bfe] h-full transition-all duration-300"
                title={`Local Cache: ${formatBytes(totalCached)}`}
              />
              <div
                style={{ width: `${dirtyPct}%` }}
                className="bg-[#d4f938] h-full transition-all duration-300"
                title={`Engine dirty cache (not the VHDX mirror queue): ${formatBytes(totalDirty)}`}
              />
            </div>

            <div className="grid grid-cols-3 gap-2 pt-2 border-t border-black/[0.04] text-[11px]">
              <div>
                <span className="text-zinc-400 block font-medium">Local Cache</span>
                <span className="font-bold text-zinc-900 mt-0.5 block">{formatBytes(totalCached)}</span>
              </div>
              <div>
                <span className="text-zinc-400 block font-medium">VHDX Mirrored</span>
                <span className="font-bold text-zinc-900 mt-0.5 block">
                  {totalMirroredChunks.toLocaleString()} / {totalVhdxChunks.toLocaleString()} chunks
                </span>
              </div>
              <div>
                <span className="text-zinc-400 block font-medium">Engine Dirty Cache</span>
                <span className="font-bold text-zinc-900 mt-0.5 block">{formatBytes(totalDirty)}</span>
              </div>
            </div>
          </div>

          {/* Bottom Breakdown Progress Bars */}
          <div className="flex flex-col gap-2.5 pt-1">
            {/* Local Cache */}
            <div className="flex items-center gap-3">
              <span className="text-lg font-extrabold text-[#18191d] w-12 text-left">
                {cachePct}<span className="text-xs font-semibold text-zinc-500">%</span>
              </span>
              <div className="flex-1 bg-zinc-100 h-2 rounded-full overflow-hidden">
                <div
                  className="h-full bg-[#a29bfe] rounded-full transition-all duration-500"
                  style={{ width: `${cachePct}%` }}
                />
              </div>
              <div className="flex items-center gap-1.5 w-24 justify-end">
                <span className="text-xs font-semibold text-zinc-700">Local Tier</span>
                <span className="w-1.5 h-1.5 rounded-full bg-[#a29bfe]" />
              </div>
            </div>

            {/* Allocated capacity ratio */}
            <div className="flex items-center gap-3">
              <span className="text-lg font-extrabold text-[#18191d] w-12 text-left">
                {allocPct}<span className="text-xs font-semibold text-zinc-500">%</span>
              </span>
              <div className="flex-1 bg-zinc-100 h-2 rounded-full overflow-hidden">
                <div
                  className="h-full bg-[#18191d] rounded-full transition-all duration-500"
                  style={{ width: `${allocPct}%` }}
                />
              </div>
              <div className="flex items-center gap-1.5 w-24 justify-end">
                <span className="text-xs font-semibold text-zinc-700">Allocated</span>
                <span className="w-1.5 h-1.5 rounded-full bg-[#18191d]" />
              </div>
            </div>

            {/* Engine dirty cache ratio */}
            <div className="flex items-center gap-3">
              <span className="text-lg font-extrabold text-[#18191d] w-12 text-left">
                {dirtyPct}<span className="text-xs font-semibold text-zinc-500">%</span>
              </span>
              <div className="flex-1 bg-zinc-100 h-2 rounded-full overflow-hidden">
                <div
                  className="h-full bg-[#d4f938] rounded-full transition-all duration-500"
                  style={{ width: `${dirtyPct}%` }}
                />
              </div>
              <div className="flex items-center gap-1.5 w-24 justify-end">
                <span className="text-xs font-semibold text-zinc-700">Engine Dirty</span>
                <span className="w-1.5 h-1.5 rounded-full bg-[#d4f938]" />
              </div>
            </div>
          </div>
        </div>

        {/* ── RIGHT COLUMN (Cards 2, 3, 4 and Wide Card 5) ──────────── */}
        <div className="lg:col-span-7 flex flex-col gap-5">
          {/* Top Half: Small Cards 2, 3 & Card 4 */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-5">
            {/* Left Sub-Column: Card 2 & Card 3 */}
            <div className="flex flex-col gap-5">
              {/* Card 2: Mounted Volumes */}
              <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[126px]">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2.5">
                    <div className="w-8 h-8 rounded-full bg-[#f4f5f8] border border-black/[0.04] flex items-center justify-center">
                      <Cpu className="w-4 h-4 text-[#18191d] stroke-[2]" />
                    </div>
                    <h3 className="font-bold text-xs text-[#18191d]">Mounted Devices</h3>
                  </div>
                  <span className={`w-2 h-2 rounded-full ${mountedDisks.length > 0 ? 'bg-emerald-500' : 'bg-zinc-300'}`} />
                </div>

                <div className="flex items-end justify-between mt-2">
                  <div className="flex items-baseline gap-1.5">
                    <span className="text-3xl font-extrabold tracking-tight text-[#18191d]">
                      {mountedDisks.length}
                    </span>
                    <span className="text-xs font-semibold text-zinc-500">/ {disks.length} Active</span>
                  </div>
                  <div className="text-right">
                    <span className="text-[11px] font-semibold text-zinc-400 block leading-tight">
                      Mount Point
                    </span>
                    <span className="text-xs font-bold text-zinc-800 font-mono">
                      {mountedDisks[0]?.drive_letter ?? 'None'}
                    </span>
                  </div>
                </div>
              </div>

              {/* Card 3: Snapshot Mirror Status */}
              <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[126px]">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2.5">
                    <div className="w-8 h-8 rounded-full bg-[#f4f5f8] border border-black/[0.04] flex items-center justify-center">
                      <Cloud className="w-4 h-4 text-[#18191d] stroke-[2]" />
                    </div>
                    <h3 className="font-bold text-xs text-[#18191d]">Snapshot Mirror</h3>
                  </div>
                  {totalVhdxChunks > 0 && (
                    <span className="px-2 py-0.5 text-[10px] font-bold rounded-full bg-[#d4f938] text-black">
                      {totalMirroredChunks} / {totalVhdxChunks}
                    </span>
                  )}
                </div>

                <div className="flex items-end justify-between mt-2">
                  <div className="flex items-baseline gap-1.5">
                    <span className="text-3xl font-extrabold tracking-tight text-[#18191d]">
                      {totalMirroredChunks}
                    </span>
                    <span className="text-xs font-semibold text-zinc-500">chunks mirrored</span>
                  </div>
                  <div className="text-right">
                    <span className="text-[11px] font-semibold text-zinc-400 block leading-tight">
                      Total Snapshot Size
                    </span>
                    <span className="text-xs font-bold text-zinc-800">
                      {totalSnapshotBytes > 0 ? formatBytes(totalSnapshotBytes) : 'Not mirrored'}
                    </span>
                  </div>
                </div>
              </div>
            </div>

            {/* Right Sub-Column: Card 4 (Integrity Status) */}
            <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[272px]">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <div className="w-8 h-8 rounded-full bg-[#f4f5f8] border border-black/[0.04] flex items-center justify-center">
                    <ShieldCheck className="w-4 h-4 text-[#18191d] stroke-[2.5]" />
                  </div>
                  <h3 className="font-bold text-xs text-[#18191d]">BLAKE3 Integrity</h3>
                </div>
                <span className={`px-2 py-0.5 text-[10px] font-extrabold rounded-full ${hasErrors ? 'bg-amber-100 text-amber-800' : 'bg-zinc-100 text-zinc-700'}`}>
                  {hasErrors ? 'Volume error' : 'On demand'}
                </span>
              </div>

              {/* Allocated chunk count; no fabricated health percentage */}
              <div className="mt-2 flex flex-col gap-1">
                <div className="flex items-baseline gap-1">
                  <span className="text-3xl font-extrabold tracking-tight text-[#18191d]">
                    {totalAllocChunks.toLocaleString()}
                  </span>
                  <span className="text-xs font-bold text-zinc-500">chunks</span>
                </div>
                <span className="text-[11px] font-semibold text-zinc-500">
                  Allocated chunks tracked across all volumes
                </span>
              </div>

              {/* Audit facts that are true without having run an audit */}
              <div className="p-3.5 bg-[#f4f5f8] rounded-2xl flex flex-col gap-2 text-xs">
                <div className="flex items-center justify-between">
                  <span className="text-zinc-500 text-[11px] font-medium">Checksum</span>
                  <span className="font-bold text-zinc-900 font-mono text-[11px]">BLAKE3-256</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-zinc-500 text-[11px] font-medium">Audit scope</span>
                  <span className="font-bold text-zinc-900 text-[11px]">Local cache only</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-zinc-500 text-[11px] font-medium">Last result</span>
                  <span className="font-bold text-zinc-900 text-[11px]">Integrity page</span>
                </div>
              </div>
            </div>
          </div>

          {/* ── CARD 5: Bottom Wide Card (Real Active Volumes Table) ── */}
          <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2.5">
                <div className="w-8 h-8 rounded-full bg-[#f4f5f8] border border-black/[0.04] flex items-center justify-center">
                  <Activity className="w-4 h-4 text-[#18191d] stroke-[2]" />
                </div>
                <h3 className="font-bold text-sm text-[#18191d]">Active Block Devices</h3>
              </div>
              <span className="text-xs font-medium text-zinc-500">
                {disks.length} Virtual Volume{disks.length === 1 ? '' : 's'} Configured
              </span>
            </div>

            {/* Real Disks Table */}
            {disks.length === 0 ? (
              <div className="py-8 flex flex-col items-center justify-center text-center gap-2 text-zinc-400">
                <HardDrive className="w-8 h-8 text-zinc-300" />
                <p className="text-sm font-bold text-zinc-700">No Volumes Configured Yet</p>
                <p className="text-xs max-w-sm">
                  Click &apos;New Virtual Disk&apos; above to allocate a Windows block storage device backed by cloud chunks.
                </p>
              </div>
            ) : (
              <div className="flex flex-col divide-y divide-black/[0.04]">
                {disks.map((disk) => {
                  const isOnline = disk.status === 'Online';
                  return (
                    <div
                      key={disk.disk_id}
                      className="py-3 flex items-center justify-between gap-4 flex-wrap"
                    >
                      <div className="flex items-center gap-3">
                        <div className={`w-9 h-9 rounded-xl flex items-center justify-center ${
                          isOnline ? 'bg-[#18191d] text-white' : 'bg-zinc-100 text-zinc-500'
                        }`}>
                          <HardDrive className="w-4 h-4 text-[#d4f938]" />
                        </div>
                        <div>
                          <div className="flex items-center gap-2">
                            <span className="font-extrabold text-xs text-[#18191d]">
                              {disk.name}
                            </span>
                            <span className={`px-2 py-0.2 rounded-full text-[10px] font-bold ${
                              isOnline ? 'bg-emerald-50 text-emerald-700' : 'bg-zinc-100 text-zinc-600'
                            }`}>
                              {disk.status} {disk.drive_letter ? `(${disk.drive_letter})` : ''}
                            </span>
                          </div>
                          <span className="text-[11px] text-zinc-400 font-medium">
                            {formatBytes(disk.capacity_bytes)} · {disk.provider_name}
                          </span>
                        </div>
                      </div>

                      {/* Direct Volume Action Buttons */}
                      <div className="flex items-center gap-2">
                        <button
                          onClick={() => onSync(disk.disk_id)}
                          title="Force chunk sync"
                          className="bg-[#f4f5f8] hover:bg-zinc-200 text-zinc-800 font-semibold text-xs px-3 py-1.5 rounded-full flex items-center gap-1.5 transition-all"
                        >
                          <RefreshCw className="w-3 h-3 text-zinc-600" />
                          <span>Sync</span>
                        </button>

                        <button
                          onClick={() => onMountToggle(disk.disk_id, disk.status)}
                          className={`font-semibold text-xs px-4 py-1.5 rounded-full transition-all ${
                            isOnline
                              ? 'bg-rose-50 hover:bg-rose-100 text-rose-700 border border-rose-200'
                              : 'bg-[#18191d] hover:bg-black text-white'
                          }`}
                        >
                          {isOnline ? 'Dismount' : 'Mount V:'}
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
