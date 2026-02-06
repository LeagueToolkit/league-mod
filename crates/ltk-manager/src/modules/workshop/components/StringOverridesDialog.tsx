import { Dialog } from "@base-ui-components/react";
import { useCallback, useEffect, useState } from "react";
import { LuPlus, LuTrash2, LuX } from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";
import { Field } from "@/components/FormField";
import { Tabs } from "@/components/Tabs";
import type { WorkshopLayer, WorkshopProject } from "@/lib/tauri";

import { useSaveStringOverrides } from "../api";

interface StringOverridesDialogProps {
  open: boolean;
  project: WorkshopProject | null;
  onClose: () => void;
}

interface OverrideEntry {
  key: string;
  value: string;
}

export function StringOverridesDialog({ open, project, onClose }: StringOverridesDialogProps) {
  const [selectedLayer, setSelectedLayer] = useState<string>("base");
  const [entries, setEntries] = useState<OverrideEntry[]>([]);
  const [hasChanges, setHasChanges] = useState(false);
  const [errors, setErrors] = useState<Record<number, string>>({});

  const saveOverrides = useSaveStringOverrides();

  // Get the currently selected layer data
  const currentLayer = project?.layers.find((l) => l.name === selectedLayer);

  // Reset entries when dialog opens or layer changes
  useEffect(() => {
    if (!currentLayer) {
      setEntries([]);
      return;
    }
    const overrides = currentLayer.stringOverrides ?? {};
    const initial = Object.entries(overrides).map(([key, value]) => ({ key, value }));
    setEntries(initial);
    setHasChanges(false);
    setErrors({});
  }, [currentLayer?.name, open]);

  // Reset selected layer when dialog opens with a new project
  useEffect(() => {
    if (open && project?.layers.length) {
      setSelectedLayer(project.layers[0].name);
    }
  }, [open, project?.path]);

  const validate = useCallback(
    (entriesToValidate: OverrideEntry[]): boolean => {
      const newErrors: Record<number, string> = {};
      const seenKeys = new Set<string>();

      for (let i = 0; i < entriesToValidate.length; i++) {
        const entry = entriesToValidate[i];
        const trimmedKey = entry.key.trim();

        if (!trimmedKey) {
          newErrors[i] = "Field name cannot be empty";
        } else if (seenKeys.has(trimmedKey)) {
          newErrors[i] = "Duplicate field name";
        }
        seenKeys.add(trimmedKey);
      }

      setErrors(newErrors);
      return Object.keys(newErrors).length === 0;
    },
    [],
  );

  function handleAddEntry() {
    setEntries((prev) => [...prev, { key: "", value: "" }]);
    setHasChanges(true);
  }

  function handleRemoveEntry(index: number) {
    setEntries((prev) => prev.filter((_, i) => i !== index));
    setErrors((prev) => {
      const next = { ...prev };
      delete next[index];
      return next;
    });
    setHasChanges(true);
  }

  function handleUpdateEntry(index: number, field: "key" | "value", val: string) {
    setEntries((prev) => {
      const updated = [...prev];
      updated[index] = { ...updated[index], [field]: val };
      return updated;
    });
    setHasChanges(true);
  }

  function handleSave() {
    if (!project || !validate(entries)) return;

    // Build the overrides map (trimming keys, filtering empty)
    const overrides: Record<string, string> = {};
    for (const entry of entries) {
      const trimmedKey = entry.key.trim();
      if (trimmedKey) {
        overrides[trimmedKey] = entry.value;
      }
    }

    saveOverrides.mutate(
      {
        projectPath: project.path,
        layerName: selectedLayer,
        stringOverrides: overrides,
      },
      {
        onSuccess: () => {
          setHasChanges(false);
        },
      },
    );
  }

  function handleClose() {
    setHasChanges(false);
    setErrors({});
    onClose();
  }

  const layers = project?.layers ?? [];
  const overrideCount = entries.length;

  return (
    <Dialog.Root open={open} onOpenChange={(isOpen) => !isOpen && handleClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm" />
        <Dialog.Popup className="fixed top-1/2 left-1/2 z-50 flex max-h-[80vh] w-full max-w-2xl -translate-x-1/2 -translate-y-1/2 flex-col rounded-xl border border-surface-600 bg-surface-800 shadow-2xl">
          {/* Header */}
          <div className="flex items-center justify-between border-b border-surface-600 px-6 py-4">
            <Dialog.Title className="text-lg font-semibold text-surface-100">
              String Overrides
            </Dialog.Title>
            <IconButton
              icon={<LuX className="h-5 w-5" />}
              variant="ghost"
              size="sm"
              onClick={handleClose}
            />
          </div>

          {/* Layer tabs */}
          {layers.length > 1 ? (
            <div className="border-b border-surface-600 px-6 pt-2">
              <Tabs.Root value={selectedLayer} onValueChange={setSelectedLayer}>
                <Tabs.List>
                  {layers.map((layer) => (
                    <Tabs.Tab key={layer.name} value={layer.name}>
                      {layer.name}
                      <OverrideBadge layer={layer} />
                    </Tabs.Tab>
                  ))}
                </Tabs.List>
              </Tabs.Root>
            </div>
          ) : null}

          {/* Content */}
          <div className="flex-1 overflow-auto px-6 py-4">
            <div className="mb-3 flex items-center justify-between">
              <p className="text-sm text-surface-400">
                {overrideCount === 0
                  ? "No string overrides configured for this layer."
                  : `${overrideCount} override${overrideCount !== 1 ? "s" : ""}`}
              </p>
              <Button
                variant="outline"
                size="sm"
                left={<LuPlus className="h-4 w-4" />}
                onClick={handleAddEntry}
              >
                Add Override
              </Button>
            </div>

            {entries.length > 0 && (
              <div className="space-y-2">
                {/* Column headers */}
                <div className="flex items-center gap-2 px-1 text-xs font-medium text-surface-400">
                  <div className="flex-1">Field Name</div>
                  <div className="flex-1">Value</div>
                  <div className="w-9" />
                </div>

                {entries.map((entry, index) => (
                  <div key={index} className="flex items-start gap-2">
                    <Field.Root className="flex-1">
                      <Field.Control
                        value={entry.key}
                        placeholder="e.g. generatedtip_spell_blindmonkqone_description"
                        hasError={!!errors[index]}
                        onChange={(e) => handleUpdateEntry(index, "key", e.target.value)}
                        onBlur={() => validate(entries)}
                      />
                      {errors[index] && (
                        <p className="text-xs text-red-500">{errors[index]}</p>
                      )}
                    </Field.Root>
                    <Field.Root className="flex-1">
                      <Field.Control
                        value={entry.value}
                        placeholder="New string value"
                        onChange={(e) => handleUpdateEntry(index, "value", e.target.value)}
                      />
                    </Field.Root>
                    <IconButton
                      icon={<LuTrash2 className="h-4 w-4" />}
                      variant="ghost"
                      size="sm"
                      className="mt-1.5 shrink-0 text-surface-400 hover:text-red-400"
                      onClick={() => handleRemoveEntry(index)}
                    />
                  </div>
                ))}
              </div>
            )}

            {entries.length === 0 && (
              <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-surface-600 py-10 text-center">
                <p className="text-sm text-surface-400">
                  String overrides let you change in-game text without replacing the full
                  stringtable file.
                </p>
                <Button
                  variant="light"
                  size="sm"
                  left={<LuPlus className="h-4 w-4" />}
                  className="mt-3"
                  onClick={handleAddEntry}
                >
                  Add your first override
                </Button>
              </div>
            )}
          </div>

          {/* Footer */}
          <div className="flex items-center justify-end gap-3 border-t border-surface-600 px-6 py-4">
            <Button variant="ghost" onClick={handleClose}>
              {hasChanges ? "Discard" : "Close"}
            </Button>
            {hasChanges && (
              <Button
                variant="filled"
                onClick={handleSave}
                loading={saveOverrides.isPending}
                disabled={Object.keys(errors).length > 0}
              >
                Save Changes
              </Button>
            )}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function OverrideBadge({ layer }: { layer: WorkshopLayer }) {
  const count = Object.keys(layer.stringOverrides ?? {}).length;
  if (count === 0) return null;
  return (
    <span className="ml-1.5 inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-brand-500/20 px-1.5 text-xs font-medium text-brand-400">
      {count}
    </span>
  );
}
