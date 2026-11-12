import React from 'react';
import {
  LayoutGrid,
  HardDrive,
  Cpu,
  ShieldCheck,
  Cloud,
  Terminal,
  Settings2,
  ArrowLeftRight,
  RotateCcw,
} from 'lucide-react';

export type NavTab =
  | 'dashboard'
  | 'disks'
  | 'providers'
  | 'cache'
  | 'transfers'
  | 'integrity'
  | 'journal'
  | 'recovery'
  | 'diagnostics'
  | 'settings';

interface SidebarProps {
  activeTab: NavTab;
  setActiveTab: (tab: NavTab) => void;
  pendingSyncCount: number;
  activeDisksCount: number;
  /** The running binary's version, reported by the backend. */
  version?: string | null;
}

export const Sidebar: React.FC<SidebarProps> = ({
  activeTab,
  setActiveTab,
  pendingSyncCount,
  activeDisksCount,
  version,
}) => {
  return (
    <aside
      id="app-sidebar"
      className="w-[230px] flex-shrink-0 bg-[#18191d] flex flex-col h-full select-none justify-between p-5 text-white"
    >
      <div className="flex flex-col gap-3 flex-1 min-h-0">
        {/* Brand Logo - 4-Petal Clover + "VirtualSSD" */}
        <div className="flex items-center gap-3 px-1.5 pt-0.5 flex-shrink-0">
          <div className="relative w-7 h-7 grid grid-cols-2 gap-0.5 flex-shrink-0">
            <span className="w-3 h-3 rounded-full bg-[#d4f938]" />
            <span className="w-3 h-3 rounded-full bg-white" />
            <span className="w-3 h-3 rounded-full bg-white" />
            <span className="w-3 h-3 rounded-full bg-[#d4f938]" />
          </div>
          <div className="flex flex-col">
            <span className="text-lg font-extrabold tracking-tight text-white leading-none">
              VirtualSSD
            </span>
            <span className="text-[9px] font-semibold text-zinc-400 tracking-wider uppercase mt-0.5">
              Block Storage
            </span>
          </div>
        </div>

        {/* Navigation Items */}
        <nav className="flex flex-col gap-2 overflow-y-auto scrollbar-none pr-1">
          {/* Dashboard */}
          <button
            id="nav-dashboard"
            onClick={() => setActiveTab('dashboard')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'dashboard'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <LayoutGrid className={`w-4 h-4 ${activeTab === 'dashboard' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Dashboard</span>
            </div>
            {activeDisksCount > 0 && (
              <span
                title={`${activeDisksCount} disk${activeDisksCount === 1 ? '' : 's'} known to the engine`}
                className="px-2 py-0.5 text-[11px] font-bold rounded-full bg-[#d4f938] text-black"
              >
                {activeDisksCount}
              </span>
            )}
          </button>

          {/* Disks & Volumes */}
          <button
            id="nav-disks"
            onClick={() => setActiveTab('disks')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'disks'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <HardDrive className={`w-4 h-4 ${activeTab === 'disks' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Disks &amp; Volumes</span>
            </div>
          </button>

          {/* Local Cache */}
          <button
            id="nav-cache"
            onClick={() => setActiveTab('cache')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'cache'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <Cpu className={`w-4 h-4 ${activeTab === 'cache' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Local Cache</span>
            </div>
          </button>

          {/* Transfer Queue */}
          <button
            id="nav-transfers"
            onClick={() => setActiveTab('transfers')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'transfers'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <ArrowLeftRight className={`w-4 h-4 ${activeTab === 'transfers' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Transfer Queue</span>
            </div>
            {pendingSyncCount > 0 && (
              <span className="px-2 py-0.5 text-[10px] font-bold rounded-full bg-[#d4f938] text-black">
                {pendingSyncCount}
              </span>
            )}
          </button>

          {/* Integrity & BLAKE3 */}
          <button
            id="nav-integrity"
            onClick={() => setActiveTab('integrity')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'integrity'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <ShieldCheck className={`w-4 h-4 ${activeTab === 'integrity' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Integrity &amp; BLAKE3</span>
            </div>
          </button>

          {/* Cloud Providers */}
          <button
            id="nav-providers"
            onClick={() => setActiveTab('providers')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'providers'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <Cloud className={`w-4 h-4 ${activeTab === 'providers' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Cloud Providers</span>
            </div>
          </button>

          {/* Disaster Recovery & WAL */}
          <button
            id="nav-recovery"
            onClick={() => setActiveTab('recovery')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'recovery'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <RotateCcw className={`w-4 h-4 ${activeTab === 'recovery' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Disaster Recovery</span>
            </div>
          </button>

          {/* Diagnostics */}
          <button
            id="nav-diagnostics"
            onClick={() => setActiveTab('diagnostics')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'diagnostics'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <Terminal className={`w-4 h-4 ${activeTab === 'diagnostics' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Diagnostics</span>
            </div>
          </button>

          {/* System Settings */}
          <button
            id="nav-settings"
            onClick={() => setActiveTab('settings')}
            className={`w-full flex items-center justify-between px-4 py-2.5 rounded-2xl transition-all duration-150 ${
              activeTab === 'settings'
                ? 'bg-white text-[#18191d] shadow-sm font-semibold'
                : 'text-[#8f919b] hover:text-white font-medium hover:bg-white/[0.05]'
            }`}
          >
            <div className="flex items-center gap-3">
              <Settings2 className={`w-4 h-4 ${activeTab === 'settings' ? 'text-black stroke-[2.5]' : 'text-[#8f919b]'}`} />
              <span className="text-[13px]">Settings</span>
            </div>
          </button>
        </nav>
      </div>

      {/* Minimalist Engine Status Footer */}
      <div className="pt-3 border-t border-white/[0.08] flex items-center justify-between text-xs text-zinc-400 mt-auto">
        <div className="flex items-center gap-2">
          <span className="w-2 h-2 rounded-full bg-[#d4f938] animate-pulse" />
          <span className="font-semibold text-zinc-300 text-[11px]">Desktop engine</span>
        </div>
        <span className="font-mono text-[10px] text-zinc-500" title={version ?? undefined}>
          {version ?? 'version unknown'}
        </span>
      </div>
    </aside>
  );
};
