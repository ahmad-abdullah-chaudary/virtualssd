import React, { useState } from 'react';
import { DiskTelemetry, ProviderRecord } from '../types';
import {
  Cloud,
  CheckCircle2,
  XCircle,
  RefreshCw,
  Trash2,
  Plus,
  ShieldCheck,
  AlertCircle,
  Key,
  MessageSquare,
  HardDrive,
  Info,
  Download,
  ChevronDown,
  ChevronUp
} from 'lucide-react';

interface ProvidersPageProps {
  disks: DiskTelemetry[];
  providers: ProviderRecord[];
  onAddGoogleDrive: (clientId: string, refreshToken: string, displayName: string) => Promise<void>;
  onAddTelegram: (botToken: string, storageChatId: number, displayName: string) => Promise<void>;
  onVerify: (providerId: string) => Promise<ProviderRecord>;
  onRemove: (providerId: string) => Promise<void>;
  onImportDisk: (providerId: string, diskId: string) => Promise<boolean>;
}

const GDriveIcon = () => (
  <svg viewBox="0 0 87.3 78" className="w-5 h-5 shrink-0" fill="none" xmlns="http://www.w3.org/2000/svg">
    <path d="M6.6 66.85l3.85 6.65c.8 1.4 1.95 2.5 3.3 3.3L27.5 53H0c0 1.55.4 3.1 1.2 4.5z" fill="#0066DA"/>
    <path d="M43.65 25L29.9 1.2c-1.35.8-2.5 1.9-3.3 3.3L1.2 48.5A9.01 9.01 0 0 0 0 53h27.5z" fill="#00AC47"/>
    <path d="M73.55 76.8c1.35-.8 2.5-1.9 3.3-3.3l1.6-2.75L86.1 57.5c.8-1.4 1.2-2.95 1.2-4.5H59.8l5.85 12.2z" fill="#EA4335"/>
    <path d="M43.65 25L57.4 1.2C56.05.4 54.5 0 52.9 0h-18.5c-1.6 0-3.15.45-4.5 1.2z" fill="#00832D"/>
    <path d="M59.8 53H27.5L13.75 76.8c1.35.8 2.9 1.2 4.5 1.2h50.8c1.6 0 3.15-.45 4.5-1.2z" fill="#2684FC"/>
    <path d="M73.4 26.5L58.6 1.2c-1.35-.8-2.9-1.2-4.5-1.2H34.4c-1.6 0-3.15.45-4.5 1.2l13.75 23.8L57.4 1.2z" fill="#FFBA00"/>
  </svg>
);

const TelegramIcon = () => (
  <svg viewBox="0 0 24 24" className="w-5 h-5 shrink-0" xmlns="http://www.w3.org/2000/svg">
    <circle cx="12" cy="12" r="12" fill="#2CA5E0"/>
    <path d="M17.21 7.48l-2.49 11.73c-.18.83-.67 1.04-1.36.65l-3.77-2.77-1.82 1.75c-.2.2-.37.37-.76.37l.27-3.81 6.97-6.3c.3-.27-.07-.42-.46-.15l-8.6 5.42-3.7-1.16c-.8-.25-.82-.8.17-1.18l14.47-5.58c.67-.24 1.25.16 1.08 1.03z" fill="white"/>
  </svg>
);

function formatBytes(bytes: number, dec = 1): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const s = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dec))} ${s[i]}`;
}

export const ProvidersPage: React.FC<ProvidersPageProps> = ({
  disks,
  providers,
  onAddGoogleDrive,
  onAddTelegram,
  onVerify,
  onRemove,
  onImportDisk,
}) => {
  const [activeForm, setActiveForm] = useState<'none' | 'gdrive' | 'telegram'>('none');
  const [verifyingId, setVerifyingId] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  // Google Drive form state
  const [gdriveDisplayName, setGdriveDisplayName] = useState('Personal Google Drive');
  const [gdriveClientId, setGdriveClientId] = useState('');
  const [gdriveRefreshToken, setGdriveRefreshToken] = useState('');

  // Telegram form state
  const [telegramDisplayName, setTelegramDisplayName] = useState('Telegram Storage Bot');
  const [telegramBotToken, setTelegramBotToken] = useState('');
  const [telegramChatId, setTelegramChatId] = useState('');

  // Remote manifest import state
  const [importProviderId, setImportProviderId] = useState('');
  const [importDiskId, setImportDiskId] = useState('');
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  const handleImportManifest = async (e: React.FormEvent) => {
    e.preventDefault();
    setImportError(null);
    const providerId = importProviderId || providers.find((p) => p.connected)?.id || '';
    if (!providerId) {
      setImportError('Select a provider first.');
      return;
    }
    const diskId = importDiskId.trim();
    if (!/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(diskId)) {
      setImportError('Enter the full disk UUID, for example 3f2a1c9e-…');
      return;
    }
    setImporting(true);
    try {
      const imported = await onImportDisk(providerId, diskId);
      if (imported) {
        setImportDiskId('');
        setImportProviderId(providerId);
      } else {
        setImportError('Import failed. See the notification for the provider error.');
      }
    } finally {
      setImporting(false);
    }
  };

  const handleVerify = async (id: string) => {
    setVerifyingId(id);
    try {
      await onVerify(id);
    } finally {
      setVerifyingId(null);
    }
  };

  const handleConnectGDrive = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!gdriveClientId.trim() || !gdriveRefreshToken.trim()) {
      setFormError('Please enter both OAuth2 Client ID and Refresh Token.');
      return;
    }
    setFormError(null);
    setIsSubmitting(true);
    try {
      await onAddGoogleDrive(gdriveClientId.trim(), gdriveRefreshToken.trim(), gdriveDisplayName.trim() || 'Google Drive');
      setActiveForm('none');
      setGdriveClientId('');
      setGdriveRefreshToken('');
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleConnectTelegram = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!telegramBotToken.trim() || !telegramChatId.trim()) {
      setFormError('Please enter both Bot Token and Storage Chat ID.');
      return;
    }
    const chatIdNum = parseInt(telegramChatId.trim(), 10);
    if (isNaN(chatIdNum)) {
      setFormError('Storage Chat ID must be a numeric integer.');
      return;
    }
    setFormError(null);
    setIsSubmitting(true);
    try {
      await onAddTelegram(telegramBotToken.trim(), chatIdNum, telegramDisplayName.trim() || 'Telegram Bot');
      setActiveForm('none');
      setTelegramBotToken('');
      setTelegramChatId('');
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="w-full flex flex-col gap-6 select-none animate-fade-in pb-8">
      {/* ── Headline Title & Actions ─────────────────────────────── */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-3xl lg:text-[38px] font-extrabold tracking-tight text-[#18191d] leading-none">
            Cloud Providers
          </h1>
          <p className="text-xs sm:text-sm font-medium text-zinc-500 mt-2">
            Configure authenticated remote storage backends for chunk distribution
          </p>
        </div>

        <div className="flex items-center gap-2.5">
          <button
            onClick={() => {
              setActiveForm(activeForm === 'gdrive' ? 'none' : 'gdrive');
              setFormError(null);
            }}
            className={`font-semibold text-xs px-4 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2 border ${
              activeForm === 'gdrive'
                ? 'bg-[#18191d] text-white border-transparent'
                : 'bg-white text-zinc-800 border-black/[0.08] hover:bg-zinc-50'
            }`}
          >
            <GDriveIcon />
            <span>Connect Google Drive</span>
            {activeForm === 'gdrive' ? <ChevronUp className="w-3.5 h-3.5 ml-1" /> : <ChevronDown className="w-3.5 h-3.5 ml-1" />}
          </button>

          <button
            onClick={() => {
              setActiveForm(activeForm === 'telegram' ? 'none' : 'telegram');
              setFormError(null);
            }}
            className={`font-semibold text-xs px-4 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2 border ${
              activeForm === 'telegram'
                ? 'bg-[#18191d] text-white border-transparent'
                : 'bg-white text-zinc-800 border-black/[0.08] hover:bg-zinc-50'
            }`}
          >
            <TelegramIcon />
            <span>Connect Telegram</span>
            {activeForm === 'telegram' ? <ChevronUp className="w-3.5 h-3.5 ml-1" /> : <ChevronDown className="w-3.5 h-3.5 ml-1" />}
          </button>
        </div>
      </div>

      {/* ── Google Drive Connection Drawer ────────────────────────── */}
      {activeForm === 'gdrive' && (
        <form onSubmit={handleConnectGDrive} className="bg-white rounded-[28px] p-6 shadow-md border border-black/[0.06] flex flex-col gap-4 animate-scale-in">
          <div className="flex items-center justify-between pb-3 border-b border-black/[0.04]">
            <div className="flex items-center gap-2.5">
              <GDriveIcon />
              <h3 className="text-base font-bold text-[#18191d]">Connect Google Drive (OAuth2 PKCE)</h3>
            </div>
            <button
              type="button"
              onClick={() => setActiveForm('none')}
              className="text-xs font-semibold text-zinc-500 hover:text-black"
            >
              Cancel
            </button>
          </div>

          {formError && (
            <div className="p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-800 text-xs flex items-center gap-2">
              <AlertCircle className="w-4 h-4 shrink-0 text-rose-600" />
              <span>{formError}</span>
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-bold text-zinc-800">Display Name</label>
              <input
                type="text"
                placeholder="e.g., Work Drive"
                value={gdriveDisplayName}
                onChange={(e) => setGdriveDisplayName(e.target.value)}
                className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-medium"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-bold text-zinc-800">OAuth2 Client ID</label>
              <input
                type="text"
                placeholder="xxxx.apps.googleusercontent.com"
                value={gdriveClientId}
                onChange={(e) => setGdriveClientId(e.target.value)}
                className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-medium"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-bold text-zinc-800">OAuth2 Refresh Token</label>
              <input
                type="password"
                placeholder="1//0xxxx…"
                value={gdriveRefreshToken}
                onChange={(e) => setGdriveRefreshToken(e.target.value)}
                className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-mono"
              />
            </div>
          </div>

          <div className="flex items-center justify-between pt-2">
            <p className="text-[11px] text-zinc-400">
              Credentials are encrypted with Windows DPAPI after verification. Scope requested: <code className="text-zinc-600 font-mono">drive.file</code>.
            </p>
            <button
              type="submit"
              disabled={isSubmitting}
              className="bg-[#18191d] hover:bg-black text-white font-semibold text-xs px-6 py-2.5 rounded-full transition-all shadow-xs disabled:opacity-50"
            >
              {isSubmitting ? 'Authenticating with Google…' : 'Authenticate & Save Provider'}
            </button>
          </div>
        </form>
      )}

      {/* ── Telegram Connection Drawer ───────────────────────────── */}
      {activeForm === 'telegram' && (
        <form onSubmit={handleConnectTelegram} className="bg-white rounded-[28px] p-6 shadow-md border border-black/[0.06] flex flex-col gap-4 animate-scale-in">
          <div className="flex items-center justify-between pb-3 border-b border-black/[0.04]">
            <div className="flex items-center gap-2.5">
              <TelegramIcon />
              <h3 className="text-base font-bold text-[#18191d]">Connect Telegram Storage (Bot API)</h3>
            </div>
            <button
              type="button"
              onClick={() => setActiveForm('none')}
              className="text-xs font-semibold text-zinc-500 hover:text-black"
            >
              Cancel
            </button>
          </div>

          {formError && (
            <div className="p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-800 text-xs flex items-center gap-2">
              <AlertCircle className="w-4 h-4 shrink-0 text-rose-600" />
              <span>{formError}</span>
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-bold text-zinc-800">Display Name</label>
              <input
                type="text"
                placeholder="e.g., Telegram Vault"
                value={telegramDisplayName}
                onChange={(e) => setTelegramDisplayName(e.target.value)}
                className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-medium"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-bold text-zinc-800">Telegram Bot Token</label>
              <input
                type="password"
                placeholder="123456789:ABCdefGHIjklmno…"
                value={telegramBotToken}
                onChange={(e) => setTelegramBotToken(e.target.value)}
                className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-mono"
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-bold text-zinc-800">Storage Chat ID</label>
              <input
                type="text"
                placeholder="-100xxxxxxxxxx or user ID"
                value={telegramChatId}
                onChange={(e) => setTelegramChatId(e.target.value)}
                className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-mono"
              />
            </div>
          </div>

          <div className="flex items-center justify-between pt-2">
            <p className="text-[11px] text-zinc-400">
              Bot token is local. Chunks are dispatched via <code className="text-zinc-600 font-mono">sendDocument</code> up to 50 MiB.
            </p>
            <button
              type="submit"
              disabled={isSubmitting}
              className="bg-[#18191d] hover:bg-black text-white font-semibold text-xs px-6 py-2.5 rounded-full transition-all shadow-xs disabled:opacity-50"
            >
              {isSubmitting ? 'Verifying Telegram Bot…' : 'Authenticate & Save Provider'}
            </button>
          </div>
        </form>
      )}

      {/* ── Remote Manifest Import ────────────────────────────────── */}
      <form onSubmit={handleImportManifest} className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col gap-4">
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-2xl bg-[#f4f5f8] flex items-center justify-center">
            <Download className="w-4 h-4 text-zinc-700" />
          </div>
          <div>
            <h3 className="text-sm font-extrabold text-[#18191d]">Import a Remote Disk Manifest</h3>
            <p className="text-[11px] text-zinc-500 mt-0.5">
              Recover a disk on this installation by fetching its published manifest. The VHDX is
              downloaded and verified on first mount; encrypted disks also need their recovery key.
            </p>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-[220px_1fr_auto] gap-3 items-end">
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-bold text-zinc-800">Source Provider</label>
            <select
              value={importProviderId}
              onChange={(e) => setImportProviderId(e.target.value)}
              className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2.5 text-xs text-zinc-900 focus:outline-none focus:ring-2 focus:ring-black/10 font-medium"
            >
              <option value="">Select a provider…</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.display_name} ({p.kind}, {p.connected ? 'online' : 'offline'})
                </option>
              ))}
            </select>
          </div>

          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-bold text-zinc-800">Disk UUID</label>
            <input
              type="text"
              spellCheck={false}
              placeholder="3f2a1c9e-1234-4abc-9def-0123456789ab"
              value={importDiskId}
              onChange={(e) => setImportDiskId(e.target.value)}
              className="bg-[#f4f5f8] border border-black/[0.05] rounded-xl px-3.5 py-2.5 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-mono"
            />
          </div>

          <button
            type="submit"
            disabled={importing || providers.length === 0}
            className="bg-[#18191d] hover:bg-black text-white font-semibold text-xs px-5 py-2.5 rounded-full transition-all shadow-xs disabled:opacity-40 disabled:cursor-not-allowed h-[38px]"
          >
            {importing ? 'Importing…' : 'Import Manifest'}
          </button>
        </div>

        {importError && (
          <div className="p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-800 text-xs flex items-center gap-2">
            <AlertCircle className="w-4 h-4 shrink-0 text-rose-600" />
            <span>{importError}</span>
          </div>
        )}
      </form>

      {/* ── Active Providers List ─────────────────────────────────── */}
      {providers.length === 0 ? (
        <div className="bg-white rounded-[28px] p-8 border border-black/[0.04] text-center flex flex-col items-center gap-3">
          <div className="w-12 h-12 rounded-2xl bg-zinc-100 flex items-center justify-center text-zinc-400">
            <Cloud className="w-6 h-6" />
          </div>
          <h3 className="font-bold text-base text-zinc-800">No Cloud Storage Providers Configured</h3>
          <p className="text-xs text-zinc-500 max-w-md">
            VirtualSSD requires an authenticated cloud backend to store block chunks. Connect your Google Drive or Telegram Bot above to begin creating virtual volumes. Chunk encryption is optional per disk and controlled by the Settings default.
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
          {providers.map((p) => {
            const isGDrive = p.kind === 'GoogleDrive';
            const attachedDisks = disks.filter(
              (d) => d.provider_name.toLowerCase().includes(p.kind.toLowerCase()) || d.provider_name.includes(p.id)
            );
            const isVerifying = verifyingId === p.id;

            return (
              <div
                key={p.id}
                className="bg-white rounded-[28px] p-6 shadow-xs border border-black/[0.04] flex flex-col justify-between gap-5 transition-all"
              >
                {/* Provider Card Top Header */}
                <div className="flex items-start justify-between">
                  <div className="flex items-center gap-3.5">
                    <div className="w-12 h-12 rounded-2xl bg-[#f4f5f8] flex items-center justify-center border border-black/[0.04]">
                      {isGDrive ? <GDriveIcon /> : <TelegramIcon />}
                    </div>
                    <div>
                      <h3 className="font-extrabold text-base text-[#18191d]">
                        {p.display_name}
                      </h3>
                      <div className="flex items-center gap-2 mt-0.5">
                        <span className="text-[11px] font-medium text-zinc-400">
                          {isGDrive ? 'Google Drive REST API' : 'Telegram Bot API'}
                        </span>
                        <span className="text-[10px] text-zinc-300">•</span>
                        <span className="text-[10px] font-mono text-zinc-400">
                          {p.id.slice(0, 8)}…
                        </span>
                      </div>
                    </div>
                  </div>

                  {p.connected ? (
                    <span className="bg-[#d4f938] text-black font-extrabold text-xs px-2.5 py-1 rounded-full flex items-center gap-1.5">
                      <CheckCircle2 className="w-3.5 h-3.5 text-black" />
                      <span>Online</span>
                    </span>
                  ) : (
                    <span className="bg-rose-100 text-rose-800 font-extrabold text-xs px-2.5 py-1 rounded-full flex items-center gap-1.5">
                      <XCircle className="w-3.5 h-3.5 text-rose-700" />
                      <span>Offline</span>
                    </span>
                  )}
                </div>

                {/* Quota / Status Details */}
                <div className="grid grid-cols-2 gap-3 p-4 bg-[#f4f5f8] rounded-2xl border border-black/[0.03] text-xs">
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Attached Volumes</span>
                    <span className="font-bold text-zinc-900 mt-0.5 block">
                      {attachedDisks.length} Volume(s)
                    </span>
                  </div>
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Storage Quota</span>
                    <span className="font-bold text-zinc-900 mt-0.5 block">
                      {p.total_quota_bytes > 0
                        ? `${formatBytes(p.used_quota_bytes)} / ${formatBytes(p.total_quota_bytes)}`
                        : isGDrive ? 'Not reported by Google' : 'Not exposed by Bot API'}
                    </span>
                  </div>
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Chunk Transport</span>
                    <span className="font-bold text-zinc-900 mt-0.5 block">
                      {isGDrive ? 'Resumable 4 MiB' : 'sendDocument 4 MiB'}
                    </span>
                  </div>
                  <div>
                    <span className="text-[11px] font-semibold text-zinc-400 block">Security Status</span>
                    <span className="font-bold text-zinc-900 mt-0.5 block">
                      Credentials DPAPI-encrypted at rest; per-disk AES-256-GCM chunk encryption
                    </span>
                  </div>
                </div>

                {/* Error Banner if offline */}
                {!p.connected && p.error_message && (
                  <div className="p-3 rounded-xl bg-rose-50 border border-rose-200 text-rose-800 text-[11px] flex items-center gap-2">
                    <AlertCircle className="w-3.5 h-3.5 shrink-0 text-rose-600" />
                    <span className="truncate">{p.error_message}</span>
                  </div>
                )}

                {/* Action Buttons */}
                <div className="flex items-center justify-between pt-1 border-t border-black/[0.03]">
                  <button
                    onClick={() => onRemove(p.id)}
                    className="text-zinc-400 hover:text-rose-600 font-semibold text-xs flex items-center gap-1.5 transition-colors"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                    <span>Disconnect</span>
                  </button>

                  <button
                    onClick={() => handleVerify(p.id)}
                    disabled={isVerifying}
                    className="bg-white border border-black/[0.08] hover:bg-zinc-50 text-zinc-800 font-semibold text-xs px-4 py-2 rounded-full transition-all shadow-xs flex items-center gap-2"
                  >
                    <RefreshCw className={`w-3.5 h-3.5 ${isVerifying ? 'animate-spin text-black' : 'text-zinc-600'}`} />
                    <span>{isVerifying ? 'Verifying…' : 'Verify Handshake'}</span>
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* ── Architecture Note ──────────────────────────────────────── */}
      <div className="bg-[#18191d] text-white rounded-[28px] p-6 shadow-md border border-white/[0.05] flex items-center justify-between">
        <div className="flex flex-col gap-1">
          <span className="text-xs font-bold text-[#d4f938] uppercase tracking-wider">Provider Adapters</span>
          <h4 className="text-base font-extrabold tracking-tight text-white">Cloud Block Storage</h4>
          <p className="text-xs text-zinc-400 max-w-xl">
            Unmount a disk, then Sync mirrors its VHDX file as 4 MiB chunks and publishes the
            manifest last. Restores verify every chunk before replacing the local file.
          </p>
        </div>

        <div className="w-12 h-12 rounded-full bg-white/10 flex items-center justify-center shrink-0">
          <ShieldCheck className="w-6 h-6 text-[#d4f938]" />
        </div>
      </div>
    </div>
  );
};
