import React, { useState, useEffect, useRef } from 'react';
import { Terminal, RefreshCw, Trash2, CheckCircle2, Activity, CircleAlert } from 'lucide-react';
import { EngineLogEntry } from '../types';

const subsystems = [
  { name: 'Virtual Disk Core',         desc: 'VHDX attach and detach through the Windows Virtual Disk API',       status: 'Present' },
  { name: 'Snapshot Mirror Engine',    desc: 'Unmounted VHDX chunk mirroring and verified restore',               status: 'Present' },
  { name: 'Chunk Encryption',          desc: 'Optional AES-256-GCM with DPAPI-protected recovery keys',           status: 'Present' },
  { name: 'BLAKE3 Integrity Engine',   desc: 'On-demand BLAKE3-256 verification of local VHDX or cached chunks',  status: 'Present' },
  { name: 'WAL Journal Manager',       desc: 'Append-only write-ahead journal with checkpoint and replay',        status: 'Present' },
  { name: 'Chunk Slicer (Block Map)',  desc: 'Translates engine sector I/O to 4 MiB chunks',                      status: 'Present' },
  { name: 'Local Cache Tier',           desc: 'Bounded local cache with clean/dirty state tracking',               status: 'Present' },
  { name: 'Cloud Provider Adapters',  desc: 'Google Drive and Telegram provider implementations',                status: 'Present' },
];

interface DiagnosticsPageProps {
  /** Real backend log entries captured by the Rust `tracing` subscriber. */
  logs: EngineLogEntry[];
  /** True when the backend log stream is connected. */
  available: boolean;
  /** Set when the log stream could not be read or subscribed to. */
  error: string | null;
  onRefresh: () => Promise<void>;
  onClear: () => Promise<void>;
}

const LEVELS = ['ALL', 'INFO', 'WARN', 'ERROR', 'DEBUG'] as const;

export const DiagnosticsPage: React.FC<DiagnosticsPageProps> = ({
  logs,
  available,
  error,
  onRefresh,
  onClear,
}) => {
  const [filter, setFilter] = useState<string>('ALL');
  const [search, setSearch] = useState('');
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoScroll && logRef.current) {
      logRef.current.scrollTop = 0;
    }
  }, [logs, autoScroll]);

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await onRefresh();
    } finally {
      setIsRefreshing(false);
    }
  };

  const handleClear = async () => {
    setIsRefreshing(true);
    try {
      await onClear();
    } finally {
      setIsRefreshing(false);
    }
  };

  const filtered = logs.filter((l) =>
    (filter === 'ALL' || l.level === filter) &&
    (search === '' ||
      l.msg.toLowerCase().includes(search.toLowerCase()) ||
      l.module.toLowerCase().includes(search.toLowerCase()))
  );

  return (
    <div className="w-full flex flex-col gap-5 select-none animate-fade-in pb-4">
      {/* ── Headline ──────────────────────────────────────────────── */}
      <div className="flex flex-col">
        <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
          Diagnostics &amp; System Logs
        </h1>
        <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
          Live backend engine log stream (Rust <code className="font-mono text-[11px]">tracing</code> subscriber) and
          storage/provider component inventory
        </p>
      </div>

      {/* ── Subsystem Health Panel ────────────────────────────────── */}
      <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-4">
        <div className="flex items-center justify-between">
          <h2 className="text-base font-extrabold tracking-tight text-[#18191d]">
            Storage Subsystem Inventory
          </h2>
          <span className="bg-[#d4f938] text-black font-extrabold text-xs px-2.5 py-1 rounded-full">
            Components compiled in
          </span>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-3 pt-1">
          {subsystems.map((sub) => (
            <div key={sub.name} className="p-4 rounded-2xl bg-[#f4f5f8] border border-black/[0.03] flex items-center justify-between gap-3 hover:bg-zinc-100/60 transition-colors">
              <div className="flex items-center gap-3">
                <div className="w-8 h-8 rounded-xl bg-white flex items-center justify-center shadow-xs flex-shrink-0">
                  <CheckCircle2 className="w-4 h-4 text-zinc-800" />
                </div>
                <div>
                  <span className="text-xs font-bold text-zinc-900 block">{sub.name}</span>
                  <span className="text-[11px] text-zinc-400 font-medium block mt-0.5">{sub.desc}</span>
                </div>
              </div>
              <div className="flex items-center gap-1.5 flex-shrink-0">
                <span className="w-1.5 h-1.5 rounded-full bg-[#d4f938] animate-pulse" />
                <span className="text-xs font-extrabold text-zinc-800">{sub.status}</span>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* ── Live Terminal Log Console ──────────────────────────────── */}
      <div className="bg-[#18191d] text-white rounded-[28px] p-6 shadow-md border border-white/[0.05] flex flex-col gap-4">
        {/* Header */}
        <div className="flex items-center justify-between flex-wrap gap-3">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-full bg-white/10 flex items-center justify-center">
              <Terminal className="w-4 h-4 text-[#d4f938]" />
            </div>
            <div>
              <h3 className="font-extrabold text-sm text-white">Live Engine Log Stream</h3>
              <div className="flex items-center gap-1.5 mt-0.5">
                <span className={`w-1.5 h-1.5 rounded-full ${available ? 'bg-[#d4f938] animate-pulse' : 'bg-amber-400'}`} />
                <span className="text-[11px] text-zinc-400 font-medium">
                  {available
                    ? `Streaming from the Rust backend · ${filtered.length} shown / ${logs.length} buffered`
                    : 'Backend log stream unavailable'}
                </span>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {/* Search */}
            <div className="relative">
              <input
                type="text"
                placeholder="Filter logs…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="bg-[#27282e] border border-white/10 rounded-full px-3 py-1.5 text-[11px] text-white placeholder-zinc-500 focus:outline-none focus:border-white/25 w-36 transition-all"
              />
            </div>

            {/* Level Filter Pills */}
            <div className="flex items-center gap-1 bg-[#27282e] p-1 rounded-full border border-white/10 text-[11px]">
              {LEVELS.map((lvl) => (
                <button
                  key={lvl}
                  onClick={() => setFilter(lvl)}
                  className={`px-2.5 py-1 rounded-full font-bold transition-all ${
                    filter === lvl ? 'bg-[#d4f938] text-black shadow-xs' : 'text-zinc-400 hover:text-white'
                  }`}
                >
                  {lvl}
                </button>
              ))}
            </div>

            <button
              onClick={handleRefresh}
              disabled={!available || isRefreshing}
              className="w-8 h-8 rounded-full bg-[#27282e] border border-white/10 flex items-center justify-center hover:bg-[#34363e] transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              title={available ? 'Re-read the backend log buffer' : 'The backend log stream is unavailable'}
            >
              <RefreshCw className={`w-3.5 h-3.5 text-zinc-300 ${isRefreshing ? 'animate-spin' : ''}`} />
            </button>

            <button
              onClick={handleClear}
              disabled={!available || isRefreshing}
              className="w-8 h-8 rounded-full bg-[#27282e] border border-white/10 flex items-center justify-center hover:bg-[#34363e] transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              title={available ? 'Clear the backend log buffer' : 'The backend log stream is unavailable'}
            >
              <Trash2 className="w-3.5 h-3.5 text-zinc-300" />
            </button>
          </div>
        </div>

        {/* Stream status */}
        <div className={`flex items-start gap-2 rounded-2xl px-3.5 py-2.5 text-[11px] leading-relaxed ${
          available
            ? 'bg-white/[0.04] border border-white/5 text-zinc-400'
            : 'bg-amber-500/10 border border-amber-500/25 text-amber-200'
        }`}>
          {available ? (
            <>
              <Activity className="w-3.5 h-3.5 mt-px flex-shrink-0 text-[#d4f938]" />
              <span>
                Entries below were emitted by the desktop backend and delivered over the Tauri event channel. The
                buffer keeps the most recent 500 records for this session.
              </span>
            </>
          ) : (
            <>
              <CircleAlert className="w-3.5 h-3.5 mt-px flex-shrink-0" />
              <span>
                {error ?? 'The backend engine log stream is unavailable.'} Run the desktop application to see real
                backend logs; the browser preview has no Rust engine to read from.
              </span>
            </>
          )}
        </div>

        {/* Console Area */}
        <div
          ref={logRef}
          className="bg-black/40 rounded-2xl p-4 font-mono text-xs flex flex-col gap-1.5 h-72 overflow-y-auto border border-white/5"
          style={{ scrollbarWidth: 'thin', scrollbarColor: '#3f4046 transparent' }}
        >
          {filtered.length === 0 ? (
            <div className="flex items-center justify-center h-full text-zinc-600 text-xs text-center px-6">
              {available
                ? logs.length === 0
                  ? 'The backend has not emitted any log entries yet.'
                  : 'No log entries match the current filter.'
                : 'No backend log stream is connected.'}
            </div>
          ) : (
            filtered.map((log) => (
              <div
                key={log.id}
                className="flex items-start gap-3 py-1 hover:bg-white/[0.03] px-2 rounded-lg transition-colors"
              >
                <span className="text-zinc-600 text-[10px] font-tabular shrink-0 pt-px">{log.ts}</span>
                <span className={`text-[10px] font-bold px-1.5 py-px rounded shrink-0 ${
                  log.level === 'INFO'  ? 'bg-blue-500/20 text-blue-300'    :
                  log.level === 'DEBUG' ? 'bg-purple-500/20 text-purple-300':
                  log.level === 'WARN'  ? 'bg-amber-500/20 text-amber-300'  :
                                          'bg-rose-500/20 text-rose-300'
                }`}>
                  {log.level}
                </span>
                <span className="text-[#d4f938] text-[10px] shrink-0">[{log.module}]</span>
                <span className="text-zinc-300 text-[10px] flex-1 leading-relaxed break-words">{log.msg}</span>
              </div>
            ))
          )}
        </div>

        {/* Bottom bar */}
        <div className="flex items-center justify-between pt-1">
          <span className="text-[11px] text-zinc-500">
            Showing {filtered.length} of {logs.length} real backend entries
          </span>
          <button
            onClick={() => setAutoScroll((v) => !v)}
            className={`text-[11px] font-semibold px-3 py-1 rounded-full transition-all ${
              autoScroll ? 'bg-[#d4f938] text-black' : 'bg-[#27282e] text-zinc-400 hover:text-white'
            }`}
          >
            {autoScroll ? '⏸ Auto-scroll On' : '▶ Auto-scroll Off'}
          </button>
        </div>
      </div>
    </div>
  );
};
