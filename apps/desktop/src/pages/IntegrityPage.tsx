import React, { useState } from 'react';
import {
  ShieldCheck,
  Play,
  CheckCircle2,
  AlertTriangle,
  Clock,
  HardDrive,
  RefreshCw,
  Lock,
} from 'lucide-react';
import { DiskTelemetry, IntegrityReport } from '../types';

interface IntegrityPageProps {
  disks: DiskTelemetry[];
  onVerify: (diskId: string) => Promise<IntegrityReport | null>;
}

interface ScrubResult {
  diskId: string;
  diskName: string;
  verified: number;
  missing: number;
  total: number;
  errors: string[];
  ts: string;
  passed: boolean;
}

export const IntegrityPage: React.FC<IntegrityPageProps> = ({ disks, onVerify }) => {
  const [running, setRunning] = useState<string | null>(null);
  const [results, setResults] = useState<ScrubResult[]>([]);

  const auditChunkCount = (disk: DiskTelemetry) =>
    disk.vhdx_chunks > 0 || disk.vhdx_file_size > 0 ? disk.vhdx_chunks : disk.allocated_chunks;

  const totalChunks = disks.reduce((acc, d) => acc + auditChunkCount(d), 0);
  const auditedChunks = results.reduce((acc, r) => acc + r.verified, 0);
  const auditedErrors = results.reduce((acc, r) => acc + r.errors.length, 0);
  const corruptionRate =
    auditedChunks > 0 ? ((auditedErrors / auditedChunks) * 100).toFixed(2) : null;

  const resultState = (r: ScrubResult): 'passed' | 'errors' | 'empty' =>
    r.errors.length > 0 ? 'errors' : r.verified === 0 && r.total > 0 ? 'empty' : 'passed';

  const startScrub = async (disk: DiskTelemetry) => {
    if (running) return;
    setRunning(disk.disk_id);
    try {
      const report = await onVerify(disk.disk_id);
      const ts = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

      if (!report) {
        setResults((prev) => [
          {
            diskId: disk.disk_id,
            diskName: disk.name,
            verified: 0,
            missing: 0,
            total: auditChunkCount(disk),
            errors: ['Audit command failed; see the notification and system log for details.'],
            ts,
            passed: false,
          },
          ...prev.slice(0, 9),
        ]);
        return;
      }

      setResults((prev) => [
        {
          diskId: report.disk_id,
          diskName: report.disk_name,
          verified: report.verified_chunks,
          missing: report.missing_chunks,
          total: report.total_chunks,
          errors: report.errors,
          ts,
          passed: report.passed,
        },
        ...prev.slice(0, 9),
      ]);
    } finally {
      setRunning(null);
    }
  };

  return (
    <div className="w-full flex flex-col gap-6 select-none animate-fade-in pb-8">
      {/* ── Headline Title & Subtitle ─────────────────────────────── */}
      <div className="flex flex-col">
        <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
          Integrity &amp; BLAKE3 Verification
        </h1>
        <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
          On-demand BLAKE3 verification of the local VHDX snapshot, or of allocated engine chunks
          when no snapshot exists. Remote chunks are not downloaded for an audit.
        </p>
      </div>

      {/* ── KPI Stat Cards Row ────────────────────────────────────── */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Hash Standard</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <ShieldCheck className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-2xl font-extrabold text-[#18191d] tracking-tight">
              BLAKE3-256
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              32-byte chunk checksums
            </p>
          </div>
        </div>

        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Checksum Errors</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <CheckCircle2 className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div className="flex items-baseline gap-2">
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {corruptionRate === null ? '—' : `${corruptionRate}%`}
            </span>
            <span className="bg-[#d4f938] text-black font-extrabold text-[10px] px-2 py-0.5 rounded-full">
              {results.length === 0
                ? 'No audits yet'
                : auditedErrors > 0
                  ? `${auditedErrors} error(s)`
                  : auditedChunks === 0
                    ? 'No local chunks audited'
                    : 'No errors in audited chunks'}
            </span>
          </div>
        </div>

        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Monitored Chunks</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <HardDrive className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-3xl font-extrabold text-[#18191d] tracking-tight">
              {totalChunks.toLocaleString()}
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              Mirrored VHDX chunks, or allocated engine chunks
            </p>
          </div>
        </div>

        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex flex-col justify-between h-[130px]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-zinc-500">Verification Scope</span>
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Lock className="w-4 h-4 text-zinc-800" />
            </div>
          </div>
          <div>
            <span className="text-2xl font-extrabold text-[#18191d] tracking-tight">
              Local VHDX / cache
            </span>
            <p className="text-[11px] font-semibold text-zinc-400 mt-0.5">
              Remote chunks are never downloaded during an audit
            </p>
          </div>
        </div>
      </div>

      {/* ── Active Volume Scrub Launchers ──────────────────────────── */}
      <div className="flex flex-col gap-4">
        <h2 className="text-lg font-extrabold tracking-tight text-[#18191d]">
          Volume Scrub &amp; Integrity Check
        </h2>

        {disks.length === 0 ? (
          <div className="bg-white rounded-[24px] p-12 text-center text-zinc-400 border border-black/[0.04] flex flex-col items-center gap-2">
            <ShieldCheck className="w-8 h-8 text-zinc-300" />
            <p className="text-sm font-bold text-zinc-700">No Volumes to Scrub</p>
            <p className="text-xs">Create a virtual disk to initiate cryptographic audits.</p>
          </div>
        ) : (
          disks.map((disk) => {
            const isScrubbing = running === disk.disk_id;
            const lastResult = results.find((r) => r.diskId === disk.disk_id);

            return (
              <div
                key={disk.disk_id}
                className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-5"
              >
                <div className="flex items-start justify-between">
                  <div className="flex items-center gap-3.5">
                    <div className="w-11 h-11 rounded-2xl bg-[#18191d] flex items-center justify-center text-white">
                      <ShieldCheck className="w-5 h-5 text-[#d4f938]" />
                    </div>
                    <div>
                      <h3 className="font-extrabold text-base text-[#18191d]">
                        {disk.name}
                      </h3>
                      <p className="text-[11px] font-mono text-zinc-400 mt-0.5">
                        {disk.disk_id} · {disk.allocated_chunks.toLocaleString()} chunks allocated
                      </p>
                    </div>
                  </div>

                  <button
                    onClick={() => startScrub(disk)}
                    disabled={isScrubbing}
                    className="bg-[#18191d] text-white hover:bg-black font-semibold text-xs px-5 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2 disabled:opacity-60"
                  >
                    {isScrubbing ? (
                      <>
                        <RefreshCw className="w-3.5 h-3.5 animate-spin text-[#d4f938]" />
                        <span>Auditing local cache…</span>
                      </>
                    ) : (
                      <>
                        <Play className="w-3.5 h-3.5 fill-current" />
                        <span>Run Local Hash Audit</span>
                      </>
                    )}
                  </button>
                </div>

                {/* Instant feedback while the backend audit runs */}
                {isScrubbing && (
                  <div className="flex flex-col gap-2 p-4 bg-[#f4f5f8] rounded-2xl border border-black/[0.03]">
                    <div className="flex justify-between text-xs font-semibold text-zinc-700">
                      <span>Verifying BLAKE3 checksums of cached chunks…</span>
                      <span className="font-mono font-bold text-zinc-900">No progress estimate</span>
                    </div>
                    <div className="w-full h-2 rounded-full overflow-hidden bg-zinc-200">
                      <div className="h-full w-1/3 bg-[#d4f938] rounded-full animate-pulse" />
                    </div>
                  </div>
                )}

                {/* Technical Integrity Specs */}
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2 border-t border-black/[0.04] text-xs">
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Checksum Engine</span>
                    <span className="font-mono font-bold text-zinc-800 mt-0.5 block">BLAKE3-256</span>
                  </div>
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Chunk Manifest</span>
                    <span className="font-bold text-zinc-800 mt-0.5 block">Versioned manifest</span>
                  </div>
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Remote Chunks</span>
                    <span className="font-bold text-zinc-800 mt-0.5 block">Not downloaded for audit</span>
                  </div>
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Last Local Audit</span>
                    <span
                      className={`font-bold mt-0.5 block ${
                        !lastResult
                          ? 'text-zinc-500'
                          : resultState(lastResult) === 'errors'
                            ? 'text-rose-700'
                            : resultState(lastResult) === 'empty'
                              ? 'text-amber-700'
                              : 'text-[#15803d]'
                      }`}
                    >
                      {!lastResult
                        ? 'Not run'
                        : resultState(lastResult) === 'errors'
                          ? `${lastResult.errors.length} error(s)`
                          : resultState(lastResult) === 'empty'
                            ? 'No local chunks audited'
                            : `${lastResult.verified}/${lastResult.total} verified, no errors`}
                    </span>
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* ── Audit Trail Log ────────────────────────────────────────── */}
      <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-4">
        <div className="flex items-center gap-2">
          <Clock className="w-4 h-4 text-zinc-500" />
          <h3 className="font-extrabold text-sm text-[#18191d]">
            Cryptographic Audit History
          </h3>
        </div>

        {results.length === 0 ? (
          <p className="text-xs text-zinc-400 py-2">
            No audit records yet. Click &apos;Run Local Hash Audit&apos; above on any volume to
            verify local BLAKE3 chunk checksums.
          </p>
        ) : (
          <div className="flex flex-col divide-y divide-black/[0.04]">
            {results.map((r, idx) => (
              <div key={`${r.diskId}-${r.ts}-${idx}`} className="py-3 flex items-start justify-between text-xs gap-4">
                <div className="flex items-start gap-3">
                  <div className="w-7 h-7 rounded-full bg-[#f4f5f8] flex items-center justify-center flex-shrink-0">
                    {resultState(r) === 'passed' ? (
                      <CheckCircle2 className="w-3.5 h-3.5 text-emerald-700" />
                    ) : resultState(r) === 'empty' ? (
                      <AlertTriangle className="w-3.5 h-3.5 text-amber-700" />
                    ) : (
                      <AlertTriangle className="w-3.5 h-3.5 text-rose-700" />
                    )}
                  </div>
                  <div className="flex flex-col gap-0.5">
                    <div>
                      <span className="font-bold text-zinc-900">{r.diskName}</span>
                      <span className="text-zinc-400 font-medium ml-2">
                        Scanned {r.verified} of {r.total} chunks
                        {r.missing > 0 ? ` · ${r.missing} not present locally` : ''}
                      </span>
                    </div>
                    {r.errors.length > 0 && (
                      <span className="text-[11px] text-rose-700 font-medium">
                        {r.errors.slice(0, 2).join('; ')}
                        {r.errors.length > 2 ? '…' : ''}
                      </span>
                    )}
                  </div>
                </div>

                <div className="flex items-center gap-3 flex-shrink-0">
                  <span className="text-[11px] font-mono text-zinc-400">{r.ts}</span>
                  <span
                    className={`font-extrabold text-[10px] px-2.5 py-0.5 rounded-full ${
                      resultState(r) === 'passed'
                        ? 'bg-[#d4f938] text-black'
                        : resultState(r) === 'empty'
                          ? 'bg-amber-100 text-amber-800'
                          : 'bg-rose-100 text-rose-800'
                    }`}
                  >
                    {resultState(r) === 'passed'
                      ? 'Verified locally'
                      : resultState(r) === 'empty'
                        ? 'Nothing cached'
                        : 'Errors'}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
