import React, { useState, useRef, useEffect } from 'react';
import {
  ChevronDown,
  HardDrive,
  Plus,
  Activity,
  CheckCircle2,
  Cpu,
  ShieldCheck,
  Zap,
  RefreshCw,
} from 'lucide-react';

interface HeaderProps {
  onOpenCreateModal?: () => void;
  onRefresh?: () => void;
  isSyncing?: boolean;
  activeDisksCount?: number;
  elevated?: boolean | null;
  onRestartElevated?: () => void;
}

export const Header: React.FC<HeaderProps> = ({
  onOpenCreateModal,
  onRefresh,
  isSyncing,
  activeDisksCount,
  elevated,
  onRestartElevated,
}) => {
  const [isLiveStatusOpen, setIsLiveStatusOpen] = useState(false);
  const popoverRef = useRef<HTMLDivElement>(null);

  // Close popover when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        setIsLiveStatusOpen(false);
      }
    };
    if (isLiveStatusOpen) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [isLiveStatusOpen]);

  return (
    <header
      id="app-header"
      className="w-full flex items-center justify-between pb-4 pt-1 select-none relative"
    >
      {/* Left: Engine & Node Identity */}
      <div className="flex items-center gap-3">
        <div className="w-10 h-10 rounded-2xl bg-[#18191d] flex items-center justify-center text-white shadow-xs">
          <HardDrive className="w-5 h-5 text-[#d4f938]" />
        </div>

        <div className="flex flex-col">
          <div className="flex items-center gap-2">
            <span className="font-extrabold text-sm text-[#18191d] tracking-tight">
              VirtualSSD Node
            </span>
            <span className="px-2 py-0.5 text-[10px] font-bold rounded-full bg-emerald-50 text-emerald-700 border border-emerald-200">
              VHDX block device
            </span>
          </div>
          <span className="text-[11px] text-zinc-500 font-medium">
            Windows Virtual Disk API · VHDX-backed volumes
          </span>
        </div>
      </div>

      {/* Right Controls: Elevation, Live Status, Create Disk */}
      <div className="flex items-center gap-3 relative" ref={popoverRef}>
        {elevated === false && onRestartElevated && (
          <button
            onClick={onRestartElevated}
            className="bg-amber-50 border border-amber-200 text-amber-800 rounded-full px-3.5 py-2 flex items-center gap-2 hover:bg-amber-100 shadow-xs transition-all text-xs font-bold"
            title="Mounting and dismounting VHDX volumes requires administrator privileges"
          >
            <ShieldCheck className="w-3.5 h-3.5" />
            <span>Restart as administrator</span>
          </button>
        )}
        {elevated === true && (
          <span
            className="bg-emerald-50 border border-emerald-200 text-emerald-800 rounded-full px-3.5 py-2 flex items-center gap-2 shadow-xs text-xs font-bold"
            title="The application can attach and detach VHDX volumes"
          >
            <ShieldCheck className="w-3.5 h-3.5" />
            <span>Administrator</span>
          </span>
        )}

        {/* Real Live Engine Status Pill & Dropdown Toggle */}
        <div className="relative">
          <button
            id="header-live-status-button"
            onClick={() => setIsLiveStatusOpen((prev) => !prev)}
            className="bg-white border border-black/[0.06] rounded-full px-3.5 py-2 flex items-center gap-2 hover:bg-zinc-50 shadow-xs transition-all text-xs font-semibold text-zinc-800"
            title="Inspect storage engine status"
          >
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500" />
            </span>
            <span>Live Status</span>
            <ChevronDown className={`w-3.5 h-3.5 text-zinc-500 transition-transform ${isLiveStatusOpen ? 'rotate-180' : ''}`} />
          </button>

          {/* Interactive Live Status Popover */}
          {isLiveStatusOpen && (
            <div className="absolute right-0 top-11 w-80 bg-white rounded-[24px] shadow-2xl border border-black/[0.08] p-5 z-50 animate-scale-in flex flex-col gap-4">
              <div className="flex items-center justify-between border-b border-black/[0.05] pb-3">
                <div className="flex items-center gap-2">
                  <div className="w-2.5 h-2.5 rounded-full bg-emerald-500 animate-pulse" />
                  <span className="font-extrabold text-xs text-[#18191d]">
                    Desktop Engine Running
                  </span>
                </div>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-emerald-50 text-emerald-700 border border-emerald-200 font-bold">
                  Live telemetry
                </span>
              </div>

              {/* Engine Metrics List */}
              <div className="flex flex-col gap-2.5 text-xs">
                <div className="flex items-center justify-between">
                  <span className="text-zinc-500 text-[11px] font-medium flex items-center gap-1.5">
                    <Cpu className="w-3.5 h-3.5 text-zinc-400" /> Block Backend
                  </span>
                  <span className="font-mono text-zinc-800 font-bold text-[11px]">
                    Windows Virtual Disk API
                  </span>
                </div>

                <div className="flex items-center justify-between">
                  <span className="text-zinc-500 text-[11px] font-medium flex items-center gap-1.5">
                    <ShieldCheck className="w-3.5 h-3.5 text-zinc-400" /> BLAKE3 Verifier
                  </span>
                  <span className="text-zinc-700 font-bold text-[11px] flex items-center gap-1">
                    <CheckCircle2 className="w-3 h-3" /> On demand
                  </span>
                </div>

                <div className="flex items-center justify-between">
                  <span className="text-zinc-500 text-[11px] font-medium flex items-center gap-1.5">
                    <Zap className="w-3.5 h-3.5 text-zinc-400" /> Chunk Cache
                  </span>
                  <span className="font-mono text-zinc-800 font-bold text-[11px]">
                    Per-disk bounded cache
                  </span>
                </div>

                <div className="flex items-center justify-between">
                  <span className="text-zinc-500 text-[11px] font-medium flex items-center gap-1.5">
                    <Activity className="w-3.5 h-3.5 text-zinc-400" /> Mounted Volumes
                  </span>
                  <span className="font-bold text-[#18191d] text-[11px]">
                    {activeDisksCount ?? 0} Volume(s) Active
                  </span>
                </div>
              </div>

              {/* Action buttons inside status popover */}
              <div className="pt-2 border-t border-black/[0.05] flex items-center justify-between gap-2">
                <button
                  onClick={() => {
                    if (onRefresh) onRefresh();
                    setIsLiveStatusOpen(false);
                  }}
                  className="flex-1 bg-[#f4f5f8] hover:bg-zinc-200 text-zinc-800 text-[11px] font-bold py-2 px-3 rounded-full flex items-center justify-center gap-1.5 transition-all"
                >
                  <RefreshCw className={`w-3 h-3 ${isSyncing ? 'animate-spin' : ''}`} />
                  <span>Refresh Telemetry</span>
                </button>
              </div>
            </div>
          )}
        </div>

        {/* Clear, Dedicated Create Disk Button */}
        <button
          id="header-create-disk-button"
          onClick={onOpenCreateModal}
          className="bg-[#18191d] hover:bg-black text-white font-bold text-xs px-4 py-2 rounded-full flex items-center gap-2 shadow-xs transition-all"
        >
          <Plus className="w-3.5 h-3.5 text-[#d4f938] stroke-[3]" />
          <span>New Disk</span>
        </button>
      </div>
    </header>
  );
};
