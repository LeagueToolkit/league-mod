import { LuLoader, LuSquare } from "react-icons/lu";

import { Button, Progress } from "@/components";
import type { OverlayProgress } from "@/lib/tauri";

import { useOverlayProgress, usePatcherError, usePatcherStatus, useStopPatcher } from "../api";

const stageLabels: Record<OverlayProgress["stage"], string> = {
  indexing: "Indexing game files...",
  collecting: "Collecting mod overrides...",
  patching: "Patching WAD files...",
  strings: "Applying string overrides...",
  complete: "Overlay built successfully!",
};

function isDeterminate(stage: OverlayProgress["stage"]) {
  return stage === "patching" || stage === "strings";
}

export function StatusBar() {
  const { data: patcherStatus } = usePatcherStatus();
  const overlayProgress = useOverlayProgress();
  const stopPatcher = useStopPatcher();
  usePatcherError();

  const isBuilding = patcherStatus?.phase === "building";
  const isRunning = patcherStatus?.running ?? false;

  // Hide when idle
  if (!isRunning && !isBuilding) return null;

  // Building overlay — show full progress
  if (isBuilding) {
    const stage = overlayProgress?.stage;
    const label = (stage && stageLabels[stage]) ?? "Preparing build...";
    const determinate = stage ? isDeterminate(stage) : false;
    const value =
      determinate && overlayProgress && overlayProgress.total > 0
        ? (overlayProgress.current / overlayProgress.total) * 100
        : null;
    const counter =
      determinate && overlayProgress && overlayProgress.total > 0
        ? `${overlayProgress.current} / ${overlayProgress.total}`
        : null;

    return (
      <div className="animate-in slide-in-from-bottom-2 border-t-2 border-brand-500 bg-brand-500/10 px-4 py-2">
        <div className="flex items-center gap-3">
          <LuLoader className="h-4 w-4 shrink-0 animate-spin text-brand-400" />
          <span className="shrink-0 text-sm font-medium text-brand-300">Building Overlay</span>
          <span className="text-sm text-surface-300">{label}</span>
          <div className="flex-1" />
          {counter && (
            <span className="shrink-0 text-sm text-surface-400 tabular-nums">{counter}</span>
          )}
        </div>
        <div className="mt-1.5 flex items-center gap-3">
          <Progress.Root value={value} className="flex-1">
            <Progress.Track size="sm">
              <Progress.Indicator />
            </Progress.Track>
          </Progress.Root>
        </div>
        {overlayProgress?.currentFile && (
          <p className="mt-1 truncate text-xs text-surface-500">{overlayProgress.currentFile}</p>
        )}
      </div>
    );
  }

  // Patcher running (post-build) — minimal indicator
  return (
    <div className="animate-in slide-in-from-bottom-2 flex items-center border-t-2 border-green-500 bg-green-500/10 px-4 py-2">
      <span className="mr-2 h-2 w-2 shrink-0 animate-pulse rounded-full bg-green-400" />
      <span className="text-sm font-medium text-green-300">Patcher running</span>
      <div className="flex-1" />
      <Button
        variant="ghost"
        size="xs"
        onClick={() => stopPatcher.mutate()}
        loading={stopPatcher.isPending}
        left={<LuSquare className="h-3 w-3" />}
      >
        Stop
      </Button>
    </div>
  );
}
