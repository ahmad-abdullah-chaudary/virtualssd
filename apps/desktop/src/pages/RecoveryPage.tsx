import React from 'react';
import { BookOpen, Database, RotateCcw, ShieldCheck, Info } from 'lucide-react';

export const RecoveryPage: React.FC = () => {
  const commitStages = [
    { num: '01', title: 'Write Sliced', desc: 'An engine API write is split into 4 MiB chunk slices', color: '#a29bfe' },
    { num: '02', title: 'WAL Transaction Begun', desc: 'A transaction id is allocated in the append-only journal', color: '#a29bfe' },
    { num: '03', title: 'Chunk Record Appended', desc: 'Chunk index, offset, length and payload are written to WAL', color: '#d4f938' },
    { num: '04', title: 'Transaction Committed', desc: 'The WAL is flushed and fsynced before the write is acknowledged', color: '#d4f938' },
    { num: '05', title: 'Cache Marked Dirty', desc: 'The chunk is stored in the persistent cache with DIRTY state', color: '#d4f938' },
    { num: '06', title: 'Reads Served Locally', desc: 'Subsequent engine reads are served from the persistent cache', color: '#a29bfe' },
    { num: '07', title: 'Engine Sync (API)', desc: 'Dirty engine chunks are uploaded by the storage-core API; the desktop UI does not expose this command', color: '#a29bfe' },
    { num: '08', title: 'Manifest Saved Atomically', desc: 'manifest.json is replaced via temp file + rename with a .prev copy', color: '#a29bfe' },
    { num: '09', title: 'WAL Checkpoint', desc: 'Committed records are checkpointed and the journal is truncated', color: '#d4f938' },
  ];

  return (
    <div className="w-full flex flex-col gap-5 select-none animate-fade-in pb-4">
      {/* ── Headline ──────────────────────────────────────────────── */}
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div className="flex flex-col">
          <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
            Journal &amp; Crash Recovery
          </h1>
          <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
            Write-ahead journal lifecycle and the recovery path applied when a disk engine is opened
          </p>
        </div>
        <span className="bg-[#f4f5f8] border border-black/[0.05] text-zinc-700 font-bold text-[11px] px-4 py-2.5 rounded-full">
          Replay runs on disk open
        </span>
      </div>

      {/* ── Scope Notice ──────────────────────────────────────────── */}
      <div className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 text-xs flex items-start gap-2.5">
        <Info className="w-4 h-4 shrink-0 mt-0.5" />
        <span>
          This timeline describes the <strong>4 MiB block-engine API path</strong>. The desktop mounts a
          Windows VHDX directly, so Windows reads and writes to the mounted volume do not pass through
          this journal or cache. The journal is opened and replayed defensively when a disk engine is
          opened, but no mounted-VHDX I/O is recorded in it. Cloud replication of a mounted disk is the
          separate, unmounted snapshot mirror.
        </span>
      </div>

      {/* ── KPI Cards ─────────────────────────────────────────────── */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Journal Mode</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Database className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <div className="flex items-baseline gap-2">
              <span className="text-2xl font-extrabold text-[#18191d] tracking-tight">WAL Mode</span>
              <span className="bg-[#d4f938] text-black font-extrabold text-[10px] px-2 py-0.5 rounded-full">Active</span>
            </div>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">Append-only log with fsync on commit</p>
          </div>
        </div>

        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Recovery Strategy</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <RotateCcw className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-2xl font-extrabold text-[#18191d] tracking-tight">Scan on Open</span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">Replayable engine writes are applied to cache</p>
          </div>
        </div>

        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Manifest Atomicity</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <BookOpen className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-2xl font-extrabold text-[#18191d] tracking-tight">Rename + .prev</span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">Previous manifest retained for fallback</p>
          </div>
        </div>
      </div>

      {/* ── Commit Timeline ──────────────────────────────────────── */}
      <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-5">
        <div>
          <h2 className="text-lg font-extrabold tracking-tight text-[#18191d]">
            Block-Engine Write &amp; Commit Path
          </h2>
          <p className="text-xs text-zinc-500 mt-1">
            This reflects the implemented storage-core journal and cache flow. Engine chunk uploads are
            not automatic and are not exposed by the desktop UI; dirty engine counters are local.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-3.5">
          {commitStages.map((stage) => (
            <div
              key={stage.num}
              className="p-4 rounded-2xl bg-[#f4f5f8] border border-black/[0.03] flex flex-col gap-2 transition-all hover:-translate-y-0.5 hover:shadow-sm"
            >
              <div className="flex items-center justify-between">
                <span className="text-xs font-extrabold text-[#18191d] flex-1 mr-2">{stage.title}</span>
                <span
                  className="w-6 h-6 rounded-full font-mono font-bold text-[11px] flex items-center justify-center flex-shrink-0"
                  style={{
                    backgroundColor: '#18191d',
                    color: stage.color === '#d4f938' ? '#d4f938' : '#a29bfe',
                  }}
                >
                  {stage.num}
                </span>
              </div>
              <p className="text-[11px] text-zinc-500 font-medium leading-relaxed">{stage.desc}</p>
              <div className="mt-auto pt-1">
                <div className="h-1 rounded-full" style={{ backgroundColor: stage.color, opacity: 0.7 }} />
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* ── Recovery Behavior Bar ─────────────────────────────────── */}
      <div className="bg-[#18191d] text-white rounded-[28px] p-6 shadow-md border border-white/[0.05] flex items-center justify-between gap-6">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs font-bold text-[#d4f938] uppercase tracking-wider">Crash Behavior</span>
          <h4 className="text-base font-extrabold tracking-tight">
            Engine writes are protected by the local WAL
          </h4>
          <p className="text-xs text-zinc-400 max-w-xl">
            When a disk engine is opened, replayable WAL transactions are applied to the local cache.
            Chunks that were not yet synchronized remain dirty and are reported as engine counters.
            Mounted VHDX data does not use this path; its cloud copy is refreshed by the next
            unmounted snapshot mirror.
          </p>
        </div>
        <div className="w-12 h-12 rounded-full bg-white/10 flex items-center justify-center flex-shrink-0">
          <ShieldCheck className="w-6 h-6 text-[#d4f938]" />
        </div>
      </div>
    </div>
  );
};
