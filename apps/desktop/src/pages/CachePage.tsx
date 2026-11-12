import React from 'react';
import { Cpu, HardDrive, Zap, Database, TrendingUp, Info } from 'lucide-react';
import { DiskTelemetry, AppStats } from '../types';

interface CachePageProps {
  disks: DiskTelemetry[];
  stats: AppStats | null;
}

function fmtBytes(b: number) {
  if (b === 0) return '0 B';
  const k = 1024, s = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(b) / Math.log(k));
  return `${parseFloat((b / k ** i).toFixed(1))} ${s[i]}`;
}

export const CachePage: React.FC<CachePageProps> = ({ disks, stats }) => {
  const totalCached   = stats?.total_cached_bytes  ?? disks.reduce((s, d) => s + d.cached_bytes, 0);
  const totalDirty    = stats?.total_dirty_bytes   ?? disks.reduce((s, d) => s + d.dirty_bytes, 0);
  const totalCapacity = stats?.total_capacity_bytes ?? disks.reduce((s, d) => s + d.capacity_bytes, 0);
  const totalPending  = disks.reduce((s, d) => s + d.pending_sync_count, 0);

  const cleanCached = Math.max(0, totalCached - totalDirty);
  const cleanShare  = totalCached > 0 ? ((cleanCached / totalCached) * 100).toFixed(1) : null;
  const capacityShare = totalCapacity > 0 ? Math.min(100, Math.round((totalCached / totalCapacity) * 100)) : 0;

  return (
    <div className="w-full flex flex-col gap-5 select-none animate-fade-in pb-4">
      {/* ── Headline ──────────────────────────────────────────────── */}
      <div className="flex flex-col">
        <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
          Block-Engine Cache
        </h1>
        <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
          Local chunk cache, dirty-chunk tracking, and journal statistics for the 4 MiB storage engine
        </p>
      </div>

      {/* ── Scope Notice ──────────────────────────────────────────── */}
      <div className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 text-xs flex items-start gap-2.5">
        <Info className="w-4 h-4 shrink-0 mt-0.5" />
        <span>
          This page reports the <strong>block-engine cache</strong> used by the storage-core chunk API. The
          desktop mounts a Windows VHDX directly, so mounted-volume I/O does not flow through this cache.
          VHDX cloud replication is the separate, unmounted snapshot mirror on the Snapshot Mirror page.
          This build does not expose an engine dirty-chunk upload command; these counters are local only.
        </span>
      </div>

      {/* ── KPI Row ───────────────────────────────────────────────── */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        {/* Local Cache Used */}
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Engine Cache Used</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Cpu className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {fmtBytes(totalCached)}
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              {capacityShare}% of total provisioned disk capacity
            </p>
          </div>
        </div>

        {/* Dirty Cache Data */}
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Engine Dirty Cache</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Database className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {fmtBytes(totalDirty)}
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              Not uploaded by mounted VHDX I/O
            </p>
          </div>
        </div>

        {/* Clean Cache Share */}
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Clean Cache Share</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Zap className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {cleanShare ?? '—'}{cleanShare !== null ? '%' : ''}
            </span>
            <span className="bg-[#d4f938] text-black font-extrabold text-[10px] px-2 py-0.5 rounded-full">
              {totalCached > 0 ? 'Live' : 'No cached data'}
            </span>
          </div>
        </div>

        {/* Pending Engine Chunks */}
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Pending Engine Chunks</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <TrendingUp className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {totalPending} <span className="text-xs font-normal text-zinc-500">chunks</span>
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              Dirty engine chunks awaiting an engine sync
            </p>
          </div>
        </div>
      </div>

      {/* ── Dark Summary Strip ────────────────────────────────────── */}
      <div className="bg-[#18191d] text-white rounded-[28px] p-6 shadow-md border border-white/[0.05] grid grid-cols-2 md:grid-cols-4 gap-6">
        {[
          { label: 'Chunk Page Size', value: '4 MiB', sub: '4,194,304 bytes' },
          { label: 'Engine Write Path', value: 'WAL + async cache', sub: 'fsync on journal commit' },
          { label: 'Chunk Mapping', value: 'Block-map slicing', sub: 'Sector offset to chunk index' },
          { label: 'Checksum on Write', value: 'BLAKE3', sub: 'Per-chunk hash recorded in manifest' },
        ].map((item) => (
          <div key={item.label} className="flex flex-col gap-1">
            <span className="text-[11px] font-semibold text-zinc-400 uppercase tracking-wider">{item.label}</span>
            <span className="text-base font-extrabold text-white tracking-tight">{item.value}</span>
            <span className="text-[11px] font-medium text-zinc-500">{item.sub}</span>
          </div>
        ))}
      </div>

      {/* ── Per-Volume Cache Breakdown ────────────────────────────── */}
      <h2 className="text-lg font-extrabold tracking-tight text-[#18191d] mt-1">
        Per-Volume Engine Cache
      </h2>

      {disks.length === 0 ? (
        <div className="bg-white rounded-[24px] p-12 text-center text-zinc-400 border border-black/[0.04] flex flex-col items-center gap-2">
          <HardDrive className="w-8 h-8 text-zinc-300" />
          <p className="text-sm font-bold text-zinc-700">No Volumes Configured</p>
          <p className="text-xs">Create a virtual disk to see engine cache allocations.</p>
        </div>
      ) : (
        disks.map((disk) => {
          const cachedPct = disk.capacity_bytes > 0
            ? Math.min(100, Math.round((disk.cached_bytes / disk.capacity_bytes) * 100))
            : 0;
          const dirtyPct = disk.cached_bytes > 0
            ? Math.min(100, Math.round((disk.dirty_bytes / disk.cached_bytes) * 100))
            : 0;

          return (
            <div key={disk.disk_id} className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-5">
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-3.5">
                  <div className="w-11 h-11 rounded-2xl bg-[#18191d] flex items-center justify-center text-white">
                    <HardDrive className="w-5 h-5 text-[#d4f938]" />
                  </div>
                  <div>
                    <h3 className="font-extrabold text-base text-[#18191d]">{disk.name}</h3>
                    <p className="text-[11px] font-mono text-zinc-400 mt-0.5">
                      {disk.disk_id} · Drive {disk.drive_letter ?? 'Unassigned'}
                    </p>
                  </div>
                </div>
                <span className={`px-3 py-1 text-xs font-bold rounded-full ${
                  disk.status === 'Online' ? 'bg-[#d4f938] text-black' : 'bg-zinc-100 text-zinc-600'
                }`}>
                  {disk.status}
                </span>
              </div>

              {/* Progress Bars */}
              <div className="grid grid-cols-1 md:grid-cols-2 gap-5 pt-2">
                <div className="flex flex-col gap-2 p-4 bg-[#f4f5f8] rounded-2xl border border-black/[0.03]">
                  <div className="flex justify-between text-xs font-semibold text-zinc-600">
                    <span>Engine Cache vs Volume Capacity</span>
                    <span className="font-bold text-zinc-900">{cachedPct}% ({fmtBytes(disk.cached_bytes)})</span>
                  </div>
                  <div className="w-full bg-zinc-200/80 h-2 rounded-full overflow-hidden">
                    <div className="h-full bg-[#a29bfe] rounded-full transition-all duration-500" style={{ width: `${cachedPct}%` }} />
                  </div>
                </div>

                <div className="flex flex-col gap-2 p-4 bg-[#f4f5f8] rounded-2xl border border-black/[0.03]">
                  <div className="flex justify-between text-xs font-semibold text-zinc-600">
                    <span>Engine Dirty Share</span>
                    <span className="font-bold text-zinc-900">{dirtyPct}% ({fmtBytes(disk.dirty_bytes)})</span>
                  </div>
                  <div className="w-full bg-zinc-200/80 h-2 rounded-full overflow-hidden">
                    <div className="h-full bg-[#d4f938] rounded-full transition-all duration-500" style={{ width: `${dirtyPct}%` }} />
                  </div>
                </div>
              </div>

              {/* Geometry Row */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2 border-t border-black/[0.04]">
                {[
                  { label: 'Chunk Page Size', value: '4,194,304 B (4 MiB)' },
                  { label: 'Engine Write Path', value: 'WAL commit, then cache' },
                  { label: 'Chunk Mapping', value: 'Sector → 4 MiB chunk' },
                  { label: 'Checksum', value: 'BLAKE3 on write' },
                ].map((item) => (
                  <div key={item.label}>
                    <span className="text-[11px] font-semibold text-zinc-400 block">{item.label}</span>
                    <span className="font-bold font-mono text-zinc-800 text-xs mt-0.5 block">{item.value}</span>
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
