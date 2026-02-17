import { Dialog, Progress } from "@/components";
import type { OverlayProgress } from "@/lib/tauri";

interface OverlayProgressDialogProps {
  open: boolean;
  overlayProgress: OverlayProgress | null;
}

const stageLabels: Record<string, string> = {
  indexing: "Indexing game files...",
  collecting: "Collecting mod overrides...",
  patching: "Patching WAD files...",
  strings: "Applying string overrides...",
};

function isDeterminate(stage: OverlayProgress["stage"]) {
  return stage === "patching" || stage === "strings";
}

export function OverlayProgressDialog({ open, overlayProgress }: OverlayProgressDialogProps) {
  const stage = overlayProgress?.stage;
  const label = (stage && stageLabels[stage]) ?? "Preparing build...";
  const determinate = stage ? isDeterminate(stage) : false;
  const value =
    determinate && overlayProgress && overlayProgress.total > 0
      ? (overlayProgress.current / overlayProgress.total) * 100
      : null;

  return (
    <Dialog.Root open={open}>
      <Dialog.Portal>
        <Dialog.Backdrop />
        <Dialog.Overlay size="sm">
          <Dialog.Header>
            <Dialog.Title>Building Overlay</Dialog.Title>
          </Dialog.Header>

          <Dialog.Body className="space-y-4">
            <Progress.Root
              value={value}
              label={label}
              valueLabel={
                determinate && overlayProgress && overlayProgress.total > 0
                  ? `${overlayProgress.current} / ${overlayProgress.total}`
                  : undefined
              }
            >
              <Progress.Track>
                <Progress.Indicator />
              </Progress.Track>
            </Progress.Root>

            {overlayProgress?.currentFile && (
              <p className="truncate text-sm text-surface-400">{overlayProgress.currentFile}</p>
            )}
          </Dialog.Body>
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
