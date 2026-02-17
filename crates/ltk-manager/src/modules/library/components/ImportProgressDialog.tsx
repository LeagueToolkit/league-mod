import { LuCircleCheck, LuCircleX } from "react-icons/lu";

import { Button, Dialog, Progress } from "@/components";
import type { BulkInstallResult, InstallProgress } from "@/lib/tauri";

interface ImportProgressDialogProps {
  open: boolean;
  onClose: () => void;
  progress: InstallProgress | null;
  result: BulkInstallResult | null;
}

export function ImportProgressDialog({
  open,
  onClose,
  progress,
  result,
}: ImportProgressDialogProps) {
  const isComplete = result !== null;

  return (
    <Dialog.Root open={open} onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop />
        <Dialog.Overlay size="sm">
          <Dialog.Header>
            <Dialog.Title>{isComplete ? "Import Complete" : "Importing Mods..."}</Dialog.Title>
          </Dialog.Header>

          <Dialog.Body className="space-y-4">
            {!isComplete && (
              <>
                {progress ? (
                  <>
                    <Progress.Root
                      value={progress.total > 0 ? (progress.current / progress.total) * 100 : 0}
                      label={`${progress.current} / ${progress.total}`}
                    >
                      <Progress.Track>
                        <Progress.Indicator />
                      </Progress.Track>
                    </Progress.Root>
                    <p className="truncate text-sm text-surface-400">{progress.currentFile}</p>
                  </>
                ) : (
                  <Progress.Root value={null} label="Preparing import...">
                    <Progress.Track>
                      <Progress.Indicator />
                    </Progress.Track>
                  </Progress.Root>
                )}
              </>
            )}

            {isComplete && result && (
              <div className="space-y-3">
                {result.installed.length > 0 && (
                  <div className="flex items-center gap-2 text-sm text-green-400">
                    <LuCircleCheck className="h-4 w-4 shrink-0" />
                    <span>
                      {result.installed.length} mod{result.installed.length !== 1 ? "s" : ""}{" "}
                      installed
                    </span>
                  </div>
                )}

                {result.failed.length > 0 && (
                  <div className="space-y-2">
                    <div className="flex items-center gap-2 text-sm text-red-400">
                      <LuCircleX className="h-4 w-4 shrink-0" />
                      <span>{result.failed.length} failed</span>
                    </div>
                    <ul className="space-y-1 pl-6">
                      {result.failed.map((err) => (
                        <li key={err.filePath} className="text-sm text-surface-400">
                          <span className="font-medium text-surface-300">{err.fileName}</span>
                          {" — "}
                          {err.message}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            )}
          </Dialog.Body>

          <Dialog.Footer>
            <Button variant="filled" size="sm" onClick={onClose}>
              {isComplete ? "Done" : "Dismiss"}
            </Button>
          </Dialog.Footer>
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
