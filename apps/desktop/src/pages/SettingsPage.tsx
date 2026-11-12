import React, { useEffect, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  Cloud,
  Database,
  HardDrive,
  Lock,
  RefreshCw,
  Save,
  Settings2,
  Shield,
  ShieldCheck,
  Timer,
} from 'lucide-react';
import { AppSettings, SettingsSnapshot } from '../types';

interface SettingsPageProps {
  settings: AppSettings;
  warning: string | null;
  elevated: boolean | null;
  onSave: (settings: AppSettings) => Promise<SettingsSnapshot>;
  onRestartElevated: () => void;
}

interface Fact {
  label: string;
  value: string;
  note: string;
}

const providerFacts: Fact[] = [
  {
    label: 'Google Drive Uploads',
    value: 'Resumable sessions, 6-day resume window',
    note: 'Session URIs are persisted under providers/gdrive_<id>/ and probed before resuming.',
  },
  {
    label: 'Google Drive Quota',
    value: 'Reported by Google',
    note: 'Unlimited plans are shown as “Not reported by Google”; the stored value is 0.',
  },
  {
    label: 'Telegram Uploads',
    value: 'Bot API sendDocument',
    note: 'The Bot API limits each document to 50 MiB; 4 MiB chunks stay below that. Replaced chunk messages are deleted after a successful re-upload.',
  },
  {
    label: 'Telegram Quota',
    value: 'Not exposed by Bot API',
    note: 'No quota endpoint exists, so the UI reports “Not exposed by Bot API” instead of a fake number.',
  },
  {
    label: 'Credential Storage',
    value: 'Windows DPAPI per user',
    note: 'Credentials are encrypted in cred_<id>.bin; providers.json contains metadata only.',
  },
];

const limitations = [
  'The VHDX is mirrored as a point-in-time snapshot and must be unmounted first. Live interception of attached-disk I/O is not implemented.',
  'Background auto-sync, scheduled scrubbing, and parallel upload tuning are not implemented; sync and audits are manual.',
  'Telegram cannot report account storage quota through the Bot API.',
  'Kernel-mode virtual disk and Windows service components are not shipped; VHDX attachment uses the Windows Virtual Disk API.',
];

const FactRow: React.FC<{ fact: Fact }> = ({ fact }) => (
  <div className="py-3.5 border-b border-black/[0.04] last:border-b-0">
    <div className="flex items-start justify-between gap-4">
      <span className="text-xs font-bold text-zinc-900">{fact.label}</span>
      <span className="text-xs font-bold text-right text-[#18191d]">{fact.value}</span>
    </div>
    <p className="text-[11px] text-zinc-500 mt-1 leading-relaxed">{fact.note}</p>
  </div>
);

const NumberField: React.FC<{
  label: string;
  value: number;
  min: number;
  max: number;
  suffix: string;
  note: string;
  onChange: (value: number) => void;
}> = ({ label, value, min, max, suffix, note, onChange }) => (
  <label className="flex flex-col gap-1.5">
    <span className="text-xs font-bold text-zinc-800">{label}</span>
    <div className="flex items-center gap-2">
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full bg-[#f4f5f8] border border-black/[0.05] rounded-2xl px-4 py-2.5 text-xs text-zinc-900 focus:outline-none focus:ring-2 focus:ring-black/10 font-medium"
      />
      <span className="text-[11px] font-semibold text-zinc-500 shrink-0">{suffix}</span>
    </div>
    <span className="text-[10px] text-zinc-400 leading-relaxed">{note}</span>
  </label>
);

export const SettingsPage: React.FC<SettingsPageProps> = ({
  settings,
  warning,
  elevated,
  onSave,
  onRestartElevated,
}) => {
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saving, setSaving] = useState(false);
  const [savedMessage, setSavedMessage] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  const update = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    setDraft((previous) => ({ ...previous, [key]: value }));
    setSavedMessage(null);
    setSaveError(null);
  };

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    setSavedMessage(null);
    setSaveError(null);

    if (!Number.isInteger(draft.cache_limit_gb) || draft.cache_limit_gb < 1 || draft.cache_limit_gb > 4096) {
      setSaveError('Cache limit must be a whole number between 1 and 4096 GiB.');
      return;
    }
    if (!Number.isInteger(draft.telemetry_poll_interval_ms) || draft.telemetry_poll_interval_ms < 500 || draft.telemetry_poll_interval_ms > 60_000) {
      setSaveError('Telemetry polling interval must be a whole number between 500 and 60000 ms.');
      return;
    }
    if (!Number.isInteger(draft.default_capacity_gb) || draft.default_capacity_gb < 1 || draft.default_capacity_gb > 65_536) {
      setSaveError('Default capacity must be a whole number between 1 and 65536 GB.');
      return;
    }

    setSaving(true);
    try {
      const snapshot = await onSave(draft);
      setDraft(snapshot.settings);
      setSavedMessage(snapshot.warning ?? 'Settings saved. New cache limits were applied to open disks.');
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="w-full flex flex-col gap-5 select-none animate-fade-in pb-4">
      {/* ── Headline ──────────────────────────────────────────────── */}
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div className="flex flex-col">
          <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
            Runtime Settings
          </h1>
          <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
            Editable defaults persisted in settings.json and applied to the running engine
          </p>
        </div>
        <span className="bg-[#f4f5f8] border border-black/[0.05] text-zinc-700 font-bold text-[11px] px-4 py-2.5 rounded-full">
          Persisted · settings.json
        </span>
      </div>

      {warning && (
        <div className="bg-amber-50 border border-amber-200 rounded-2xl p-4 flex items-start gap-3">
          <AlertTriangle className="w-4 h-4 text-amber-700 flex-shrink-0 mt-0.5" />
          <div>
            <p className="text-xs font-bold text-amber-900">Stored settings could not be loaded</p>
            <p className="text-[11px] text-amber-800 mt-0.5">{warning}</p>
          </div>
        </div>
      )}

      {/* ── Editable Settings Form ────────────────────────────────── */}
      <form onSubmit={handleSave} className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-5">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
            <Settings2 className="w-4 h-4 text-zinc-800" />
          </div>
          <div>
            <h2 className="text-base font-extrabold tracking-tight text-[#18191d]">Storage &amp; Telemetry</h2>
            <p className="text-[11px] text-zinc-500">Values are validated by the desktop backend before they are saved.</p>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
          <NumberField
            label="Per-disk Cache Limit"
            value={draft.cache_limit_gb}
            min={1}
            max={4096}
            suffix="GiB"
            note="Applies to open disks immediately; clean/committed chunks may be evicted. Dirty chunks are never evicted."
            onChange={(value) => update('cache_limit_gb', value)}
          />

          <NumberField
            label="Telemetry Polling Interval"
            value={draft.telemetry_poll_interval_ms}
            min={500}
            max={60_000}
            suffix="ms"
            note="How often the UI refreshes disk, provider, and stats data."
            onChange={(value) => update('telemetry_poll_interval_ms', value)}
          />

          <NumberField
            label="Default New-Disk Capacity"
            value={draft.default_capacity_gb}
            min={1}
            max={65_536}
            suffix="GB"
            note="Pre-selected in the Create Disk dialog. Each disk can still choose a different size."
            onChange={(value) => update('default_capacity_gb', value)}
          />

          <div className="flex flex-col gap-2 justify-center">
            <label className="flex items-start gap-3 p-3.5 rounded-2xl bg-[#f4f5f8] border border-black/[0.04] cursor-pointer">
              <input
                type="checkbox"
                checked={draft.encrypt_new_disks}
                onChange={(e) => update('encrypt_new_disks', e.target.checked)}
                className="mt-0.5 w-4 h-4 accent-[#18191d]"
              />
              <span>
                <span className="text-xs font-bold text-zinc-900 flex items-center gap-1.5">
                  <Lock className="w-3.5 h-3.5" /> Encrypt new disks
                </span>
                <span className="text-[10px] text-zinc-500 block mt-0.5 leading-relaxed">
                  AES-256-GCM with a per-disk key protected by Windows DPAPI. Fixed at creation; existing disks are unchanged.
                </span>
              </span>
            </label>

            <label className="flex items-start gap-3 p-3.5 rounded-2xl bg-[#f4f5f8] border border-black/[0.04] cursor-pointer">
              <input
                type="checkbox"
                checked={draft.verify_after_sync}
                onChange={(e) => update('verify_after_sync', e.target.checked)}
                className="mt-0.5 w-4 h-4 accent-[#18191d]"
              />
              <span>
                <span className="text-xs font-bold text-zinc-900 flex items-center gap-1.5">
                  <ShieldCheck className="w-3.5 h-3.5" /> Verify after every snapshot sync
                </span>
                <span className="text-[10px] text-zinc-500 block mt-0.5 leading-relaxed">
                  Runs a local BLAKE3 audit of the VHDX snapshot immediately after the manifest is published.
                </span>
              </span>
            </label>
          </div>
        </div>

        {saveError && (
          <div className="p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-800 text-xs flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 shrink-0 text-rose-600" />
            <span>{saveError}</span>
          </div>
        )}

        {savedMessage && (
          <div className="p-3 rounded-xl bg-emerald-50 border border-emerald-200 text-emerald-800 text-xs flex items-center gap-2">
            <CheckCircle2 className="w-4 h-4 shrink-0 text-emerald-600" />
            <span>{savedMessage}</span>
          </div>
        )}

        <div className="flex items-center justify-between pt-1 border-t border-black/[0.04]">
          <button
            type="button"
            onClick={() => {
              setDraft(settings);
              setSavedMessage(null);
              setSaveError(null);
            }}
            className="text-zinc-600 hover:text-black font-semibold text-xs px-2 py-2 rounded-full transition-colors flex items-center gap-1.5"
          >
            <RefreshCw className="w-3.5 h-3.5" /> Reset to saved values
          </button>

          <button
            type="submit"
            disabled={saving}
            className="bg-[#18191d] hover:bg-black text-white font-bold text-xs px-5 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2 disabled:opacity-50"
          >
            <Save className="w-3.5 h-3.5 text-[#d4f938]" />
            <span>{saving ? 'Saving…' : 'Save Settings'}</span>
          </button>
        </div>
      </form>

      {/* ── Elevation ─────────────────────────────────────────────── */}
      <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex items-center justify-between flex-wrap gap-4">
        <div className="flex items-center gap-3">
          <div className={`w-9 h-9 rounded-2xl flex items-center justify-center ${elevated ? 'bg-emerald-50' : 'bg-amber-50'}`}>
            <ShieldCheck className={`w-5 h-5 ${elevated ? 'text-emerald-600' : 'text-amber-600'}`} />
          </div>
          <div>
            <p className="text-sm font-extrabold text-[#18191d]">
              {elevated === null ? 'Checking administrator status…' : elevated ? 'Running as administrator' : 'Running as a standard user'}
            </p>
            <p className="text-[11px] text-zinc-500 mt-0.5">
              Mounting, dismounting, and restoring a VHDX requires administrator privileges. The app
              starts without elevation so diagnostics and provider setup always work.
            </p>
          </div>
        </div>

        {!elevated && (
          <button
            onClick={onRestartElevated}
            className="bg-amber-500 hover:bg-amber-600 text-black font-bold text-xs px-5 py-2.5 rounded-full transition-all shadow-xs"
          >
            Restart as administrator
          </button>
        )}
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-5 items-start">
        {/* ── Providers & Credentials ────────────────────────────── */}
        <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04]">
          <div className="flex items-center gap-3 mb-2">
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Cloud className="w-4 h-4 text-zinc-800" />
            </div>
            <h2 className="text-base font-extrabold tracking-tight text-[#18191d]">
              Providers &amp; Credentials
            </h2>
          </div>
          {providerFacts.map((fact) => (
            <FactRow key={fact.label} fact={fact} />
          ))}
        </div>

        {/* ── Current Limitations ────────────────────────────────── */}
        <div className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04]">
          <div className="flex items-center gap-3 mb-3">
            <div className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center">
              <Settings2 className="w-4 h-4 text-zinc-800" />
            </div>
            <h2 className="text-base font-extrabold tracking-tight text-[#18191d]">
              Current Limitations
            </h2>
          </div>
          <ul className="flex flex-col gap-2">
            {limitations.map((item) => (
              <li key={item} className="text-[11px] text-zinc-600 flex items-start gap-2">
                <span className="w-1.5 h-1.5 rounded-full bg-zinc-400 mt-1.5 flex-shrink-0" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>

      {/* ── Security Summary ──────────────────────────────────────── */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex items-center gap-3">
          <Shield className="w-5 h-5 text-zinc-800" />
          <div>
            <p className="text-xs font-bold text-zinc-900">DPAPI credentials</p>
            <p className="text-[11px] text-zinc-500">Current Windows user scope</p>
          </div>
        </div>
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex items-center gap-3">
          <Database className="w-5 h-5 text-zinc-800" />
          <div>
            <p className="text-xs font-bold text-zinc-900">Atomic file writes</p>
            <p className="text-[11px] text-zinc-500">Temp file + rename for settings, manifests, and credentials</p>
          </div>
        </div>
        <div className="bg-white rounded-[24px] p-5 shadow-xs border border-black/[0.04] flex items-center gap-3">
          <Timer className="w-5 h-5 text-zinc-800" />
          <div>
            <p className="text-xs font-bold text-zinc-900">Snapshot mirroring</p>
            <p className="text-[11px] text-zinc-500">Manual, unmounted-only VHDX chunk upload</p>
          </div>
        </div>
      </div>
    </div>
  );
};
