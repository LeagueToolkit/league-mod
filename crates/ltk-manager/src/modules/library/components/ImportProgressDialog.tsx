import { LuCircleCheck, LuCircleX } from "react-icons/lu";

import { Button, Dialog } from "@/components";
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
    <Dialog.Root open={open} onOpenChange={(open) => !open && isComplete && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop />
        <Dialog.Overlay size="sm">
          <Dialog.Header>
            <Dialog.Title>{isComplete ? "Import Complete" : "Importing Mods..."}</Dialog.Title>
          </Dialog.Header>

          <Dialog.Body className="space-y-4">
            {!isComplete && progress && (
              <>
                <div className="space-y-2">
                  <div className="flex justify-between text-sm text-surface-300">
                    <span>
                      {progress.current} / {progress.total}
                    </span>
                  </div>
                  <div className="h-2 overflow-hidden rounded-full bg-surface-700">
                    <div
                      className="h-full rounded-full bg-brand-500 transition-all duration-300"
                      style={{ width: `${(progress.current / progress.total) * 100}%` }}
                    />
                  </div>
                </div>
                <p className="truncate text-sm text-surface-400">{progress.currentFile}</p>
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

          {isComplete && (
            <Dialog.Footer>
              <Button variant="filled" size="sm" onClick={onClose}>
                Done
              </Button>
            </Dialog.Footer>
          )}
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
