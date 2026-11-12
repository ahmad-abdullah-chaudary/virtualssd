import React, { useState, useEffect, useRef } from 'react';
import { X, HardDrive, CheckCircle2, AlertTriangle, ShieldCheck } from 'lucide-react';
import { ProviderRecord } from '../types';

interface CreateDiskModalProps {
  isOpen: boolean;
  onClose: () => void;
  onCreate: (name: string, providerAccountId: string, capacityGb: number) => Promise<void>;
  providers: ProviderRecord[];
  defaultCapacityGb: number;
  encryptNewDisks: boolean;
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

const CAPACITY_PRESETS = [
  { label: '50 GB',  value: 50 },
  { label: '100 GB', value: 100 },
  { label: '250 GB', value: 250 },
  { label: '500 GB', value: 500 },
  { label: '1 TB',   value: 1024 },
];

export const CreateDiskModal: React.FC<CreateDiskModalProps> = ({
  isOpen,
  onClose,
  onCreate,
  providers,
  defaultCapacityGb,
  encryptNewDisks,
}) => {
  const [name, setName] = useState('');
  const [selectedProviderId, setSelectedProviderId] = useState<string>('');
  const [capacityGb, setCapacityGb] = useState(100);
  const [creating, setCreating] = useState(false);
  const [done, setDone] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  // The form is reset when the dialog *opens*. It must not be reset when the
  // provider list changes: the application polls the backend, so `providers`
  // is a new array on every poll, and resetting on that would wipe the name
  // the user had typed and any error that had been shown.
  const wasOpen = useRef(isOpen);
  useEffect(() => {
    if (isOpen && !wasOpen.current) {
      setName('');
      setCapacityGb(defaultCapacityGb);
      setCreating(false);
      setDone(false);
      setCreateError(null);
      setSelectedProviderId('');
    }
    wasOpen.current = isOpen;
  }, [isOpen, defaultCapacityGb]);

  // Select the first connected provider, but only while the user has not
  // already chosen one. This runs on every provider refresh without
  // disturbing the rest of the form.
  useEffect(() => {
    if (!isOpen) return;
    setSelectedProviderId((current) => {
      if (current && providers.some((p) => p.id === current)) return current;
      const firstOnline = providers.find((p) => p.connected);
      if (firstOnline) return firstOnline.id;
      return providers.length > 0 ? providers[0].id : '';
    });
  }, [isOpen, providers]);

  if (!isOpen) return null;

  const selectedProvider = providers.find((p) => p.id === selectedProviderId);
  const canSubmit = name.trim().length > 0 && selectedProvider != null && selectedProvider.connected && !creating;

  const handleCreate = async () => {
    if (!canSubmit) return;
    setCreating(true);
    setCreateError(null);
    try {
      await onCreate(name.trim(), selectedProviderId, capacityGb);
      setDone(true);
      setTimeout(() => onClose(), 800);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setCreateError(msg);
      setCreating(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Blurred Backdrop */}
      <div className="absolute inset-0 bg-black/50 backdrop-blur-sm" onClick={onClose} />

      {/* Modal Dialog */}
      <div className="relative bg-white rounded-[32px] p-7 shadow-2xl border border-black/[0.04] max-w-lg w-full z-10 flex flex-col gap-6 animate-scale-in">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-2xl bg-[#18191d] flex items-center justify-center text-white">
              <HardDrive className="w-5 h-5 text-[#d4f938]" />
            </div>
            <div>
              <h2 className="text-xl font-extrabold text-[#18191d] tracking-tight">
                Create Virtual Disk
              </h2>
              <p className="text-xs text-zinc-500 mt-0.5">
                Allocate a Windows-visible VHDX volume. After you unmount it, use Sync to mirror
                its snapshot to the selected cloud provider.
              </p>
            </div>
          </div>

          <button
            onClick={onClose}
            className="w-8 h-8 rounded-full bg-[#f4f5f8] flex items-center justify-center text-zinc-500 hover:text-black transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content Form */}
        <div className="flex flex-col gap-4">
          {/* Volume Name */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-bold text-zinc-800">Volume Name</label>
            <input
              type="text"
              placeholder="e.g., Secure Cloud Disk"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="w-full bg-[#f4f5f8] border border-black/[0.05] rounded-2xl px-4 py-2.5 text-xs text-zinc-900 placeholder-zinc-400 focus:outline-none focus:ring-2 focus:ring-black/10 font-medium"
            />
          </div>

          {/* Cloud Storage Provider Selection */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-bold text-zinc-800">Backing Cloud Provider</label>
            
            {providers.length === 0 ? (
              <div className="p-4 rounded-2xl bg-amber-50 border border-amber-200 text-amber-900 flex flex-col gap-2">
                <div className="flex items-center gap-2 font-bold text-xs text-amber-800">
                  <AlertTriangle className="w-4 h-4 text-amber-600 shrink-0" />
                  <span>No Cloud Providers Configured</span>
                </div>
                <p className="text-[11px] text-amber-700 leading-relaxed">
                  Per VirtualSSD architecture, every disk requires a verified Google Drive OAuth2 account or Telegram Bot API backend. Please go to the <strong>Cloud Providers</strong> tab to configure your credentials first.
                </p>
              </div>
            ) : (
              <div className="flex flex-col gap-2 max-h-48 overflow-y-auto pr-1">
                {providers.map((p) => {
                  const isSelected = selectedProviderId === p.id;
                  const isGDrive = p.kind === 'GoogleDrive';
                  return (
                    <div
                      key={p.id}
                      onClick={() => setSelectedProviderId(p.id)}
                      className={`p-3.5 rounded-2xl border cursor-pointer flex items-center justify-between transition-all ${
                        isSelected
                          ? 'border-[#18191d] bg-white ring-2 ring-black/10 shadow-xs'
                          : 'border-black/[0.05] bg-[#f4f5f8] hover:border-black/15'
                      } ${!p.connected ? 'opacity-60' : ''}`}
                    >
                      <div className="flex items-center gap-3 min-w-0">
                        {isGDrive ? <GDriveIcon /> : <TelegramIcon />}
                        <div className="min-w-0">
                          <div className="flex items-center gap-2">
                            <span className="text-xs font-bold text-zinc-900 truncate">
                              {p.display_name}
                            </span>
                            <span className="text-[10px] text-zinc-400">
                              ({p.kind})
                            </span>
                          </div>
                          <span className="text-[10px] text-zinc-500 font-mono block truncate">
                            ID: {p.id.slice(0, 8)}…
                          </span>
                        </div>
                      </div>

                      <div className="flex items-center gap-2 shrink-0">
                        {p.connected ? (
                          <span className="bg-emerald-100 text-emerald-800 text-[10px] font-bold px-2 py-0.5 rounded-full">
                            Online
                          </span>
                        ) : (
                          <span className="bg-rose-100 text-rose-800 text-[10px] font-bold px-2 py-0.5 rounded-full" title={p.error_message || 'Offline'}>
                            Offline
                          </span>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          {/* Capacity Preset Buttons */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-bold text-zinc-800">Volume Capacity (GB)</label>
            <div className="grid grid-cols-5 gap-2">
              {CAPACITY_PRESETS.map((p) => (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => setCapacityGb(p.value)}
                  className={`py-2 rounded-xl text-xs font-bold transition-all ${
                    capacityGb === p.value
                      ? 'bg-[#18191d] text-white shadow-xs'
                      : 'bg-[#f4f5f8] text-zinc-600 hover:text-black hover:bg-zinc-200'
                  }`}
                >
                  {p.label}
                </button>
              ))}
            </div>
            <input
              type="number"
              min={1}
              max={65536}
              value={capacityGb}
              onChange={(e) => {
                const next = Number(e.target.value);
                if (Number.isFinite(next)) setCapacityGb(next);
              }}
              className="mt-1 w-full bg-[#f4f5f8] border border-black/[0.05] rounded-2xl px-4 py-2.5 text-xs text-zinc-900 focus:outline-none focus:ring-2 focus:ring-black/10 font-medium"
            />
            <span className="text-[10px] text-zinc-400 mt-0.5">
              The default comes from Settings. A dynamic local VHDX of this virtual size is
              created when the disk is first mounted.
            </span>
          </div>

          {/* Encryption State */}
          <div className={`p-3.5 rounded-2xl border flex items-start gap-2.5 ${
            encryptNewDisks
              ? 'bg-emerald-50 border-emerald-200 text-emerald-900'
              : 'bg-[#f4f5f8] border-black/[0.05] text-zinc-700'
          }`}>
            <ShieldCheck className={`w-4 h-4 mt-0.5 shrink-0 ${encryptNewDisks ? 'text-emerald-700' : 'text-zinc-400'}`} />
            <div>
              <p className="text-[11px] font-bold">
                {encryptNewDisks ? 'Client-side encryption is enabled for this disk' : 'Chunk encryption is disabled for this disk'}
              </p>
              <p className="text-[10px] mt-0.5 leading-relaxed">
                {encryptNewDisks
                  ? 'VHDX chunks are sealed with AES-256-GCM. Export the recovery key after creation and keep it safe; it is not recoverable from the provider.'
                  : 'Change this default on the Settings page. Encryption is fixed per disk and cannot be enabled later without re-uploading the snapshot.'}
              </p>
            </div>
          </div>
        </div>

        {createError && (
          <div className="px-4 py-2.5 rounded-2xl bg-rose-50 border border-rose-200 text-rose-700 text-[11px] font-medium">
            {createError}
          </div>
        )}

        {/* Footer Actions */}
        <div className="flex items-center justify-between pt-2 border-t border-black/[0.04]">
          <div className="flex items-center gap-1.5 text-zinc-400 text-[11px]">
            <ShieldCheck className="w-3.5 h-3.5 text-emerald-600" />
            <span>Administrator required to mount</span>
          </div>

          <div className="flex items-center gap-3">
            <button
              onClick={onClose}
              className="px-4 py-2 rounded-full text-xs font-semibold text-zinc-600 hover:text-black transition-colors"
            >
              Cancel
            </button>

            <button
              onClick={handleCreate}
              disabled={!canSubmit}
              className="bg-[#18191d] hover:bg-black text-white font-semibold text-xs px-5 py-2.5 rounded-full transition-all shadow-xs flex items-center gap-2 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {done ? (
                <>
                  <CheckCircle2 className="w-4 h-4 text-[#d4f938]" />
                  <span>Volume Created</span>
                </>
              ) : creating ? (
                <span>Creating Volume…</span>
              ) : (
                <span>Create Virtual Volume</span>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
