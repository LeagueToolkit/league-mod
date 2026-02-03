import { Dialog } from "@base-ui-components/react";
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import {
  LuCheck,
  LuCircleAlert,
  LuFolderOpen,
  LuPackage,
  LuTriangleAlert,
  LuX,
} from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";
import { RadioGroup } from "@/components/RadioGroup";
import type { PackResult, ValidationResult, WorkshopProject } from "@/lib/tauri";

interface PackDialogProps {
  open: boolean;
  project: WorkshopProject | null;
  validation: ValidationResult | null;
  validationLoading: boolean;
  onClose: () => void;
  onPack: (format: "modpkg" | "fantome") => void;
  isPacking: boolean;
  packResult: PackResult | null;
}

export function PackDialog({
  open,
  project,
  validation,
  validationLoading,
  onClose,
  onPack,
  isPacking,
  packResult,
}: PackDialogProps) {
  const [format, setFormat] = useState<"modpkg" | "fantome">("modpkg");

  if (!project) return null;

  const hasErrors = validation && validation.errors.length > 0;
  const hasWarnings = validation && validation.warnings.length > 0;

  return (
    <Dialog.Root open={open} onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm" />
        <Dialog.Popup className="fixed top-1/2 left-1/2 z-50 w-full max-w-lg -translate-x-1/2 -translate-y-1/2 rounded-xl border border-surface-600 bg-surface-800 shadow-2xl">
          <div className="flex items-center justify-between border-b border-surface-600 px-6 py-4">
            <Dialog.Title className="text-lg font-semibold text-surface-100">
              Pack {project.displayName}
            </Dialog.Title>
            <IconButton
              icon={<LuX className="h-5 w-5" />}
              variant="ghost"
              size="sm"
              onClick={onClose}
            />
          </div>

          <div className="px-6 py-4">
            {packResult ? (
              <div className="space-y-4">
                <div className="flex flex-col items-center py-4 text-center">
                  <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-green-500/20">
                    <LuCheck className="h-8 w-8 text-green-400" />
                  </div>
                  <h3 className="text-lg font-semibold text-surface-100">Package Created</h3>
                  <p className="mt-2 text-sm font-medium text-surface-200">{packResult.fileName}</p>
                  <p className="mt-1 max-w-sm text-xs break-all text-surface-400">
                    {packResult.outputPath}
                  </p>
                </div>
              </div>
            ) : (
              <div className="space-y-4">
                {validationLoading ? (
                  <div className="flex items-center gap-2 text-surface-400">
                    <div className="h-4 w-4 animate-spin rounded-full border-2 border-brand-500 border-t-transparent" />
                    Validating project...
                  </div>
                ) : validation ? (
                  <div className="space-y-3">
                    {hasErrors && (
                      <div className="space-y-2">
                        <div className="flex items-center gap-2 text-red-400">
                          <LuCircleAlert className="h-4 w-4" />
                          <span className="text-sm font-medium">
                            {validation.errors.length} error{validation.errors.length !== 1 && "s"}
                          </span>
                        </div>
                        <ul className="space-y-1 pl-6 text-sm text-red-300">
                          {validation.errors.map((error, i) => (
                            <li key={i}>• {error}</li>
                          ))}
                        </ul>
                      </div>
                    )}

                    {hasWarnings && (
                      <div className="space-y-2">
                        <div className="flex items-center gap-2 text-yellow-400">
                          <LuTriangleAlert className="h-4 w-4" />
                          <span className="text-sm font-medium">
                            {validation.warnings.length} warning
                            {validation.warnings.length !== 1 && "s"}
                          </span>
                        </div>
                        <ul className="space-y-1 pl-6 text-sm text-yellow-300">
                          {validation.warnings.map((warning, i) => (
                            <li key={i}>• {warning}</li>
                          ))}
                        </ul>
                      </div>
                    )}

                    {validation.valid && !hasWarnings && (
                      <div className="flex items-center gap-2 text-green-400">
                        <LuCheck className="h-4 w-4" />
                        <span className="text-sm">Project is valid</span>
                      </div>
                    )}
                  </div>
                ) : null}

                <RadioGroup.Root
                  value={format}
                  onValueChange={(value: unknown) => setFormat(value as "modpkg" | "fantome")}
                >
                  <RadioGroup.Label>Output Format</RadioGroup.Label>
                  <RadioGroup.Options>
                    <RadioGroup.Card
                      value="modpkg"
                      title=".modpkg"
                      description="Full support for layers and metadata"
                    />
                    <RadioGroup.Card
                      value="fantome"
                      title=".fantome"
                      description="Legacy format (base layer only)"
                    />
                  </RadioGroup.Options>
                </RadioGroup.Root>

                {format === "fantome" && project.layers.length > 1 && (
                  <div className="flex items-start gap-2 rounded-lg border border-yellow-500/30 bg-yellow-500/10 p-3 text-sm">
                    <LuTriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-yellow-400" />
                    <div className="text-yellow-300">
                      This project has {project.layers.length} layers, but Fantome format only
                      supports the base layer. Other layers will not be included.
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>

          <div className="flex justify-end gap-3 border-t border-surface-600 px-6 py-4">
            {packResult ? (
              <>
                <Button variant="ghost" onClick={onClose}>
                  Close
                </Button>
                <Button
                  variant="filled"
                  left={<LuFolderOpen className="h-4 w-4" />}
                  onClick={async () => {
                    try {
                      await invoke("reveal_in_explorer", { path: packResult.outputPath });
                    } catch (error) {
                      console.error("Failed to open folder:", error);
                    }
                  }}
                >
                  Show in Explorer
                </Button>
              </>
            ) : (
              <>
                <Button variant="ghost" onClick={onClose}>
                  Cancel
                </Button>
                <Button
                  variant="filled"
                  left={<LuPackage className="h-4 w-4" />}
                  onClick={() => onPack(format)}
                  loading={isPacking}
                  disabled={hasErrors || validationLoading}
                >
                  {isPacking ? "Packing..." : "Pack"}
                </Button>
              </>
            )}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
