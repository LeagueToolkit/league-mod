import { Dialog } from "@base-ui-components/react";
import { useCallback, useEffect, useState } from "react";
import { LuGlobe, LuPlus, LuTrash2, LuX } from "react-icons/lu";

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

// Common League of Legends locales
const LOCALES = [
  { value: "default", label: "Default (All Locales)" },
  { value: "en_us", label: "English (US)" },
  { value: "en_gb", label: "English (UK)" },
  { value: "en_au", label: "English (AU)" },
  { value: "en_ph", label: "English (PH)" },
  { value: "en_sg", label: "English (SG)" },
  { value: "ko_kr", label: "Korean" },
  { value: "ja_jp", label: "Japanese" },
  { value: "zh_cn", label: "Chinese (Simplified)" },
  { value: "zh_tw", label: "Chinese (Traditional)" },
  { value: "es_es", label: "Spanish (EU)" },
  { value: "es_mx", label: "Spanish (LATAM)" },
  { value: "es_ar", label: "Spanish (AR)" },
  { value: "pt_br", label: "Portuguese (BR)" },
  { value: "fr_fr", label: "French" },
  { value: "de_de", label: "German" },
  { value: "it_it", label: "Italian" },
  { value: "pl_pl", label: "Polish" },
  { value: "ro_ro", label: "Romanian" },
  { value: "el_gr", label: "Greek" },
  { value: "ru_ru", label: "Russian" },
  { value: "tr_tr", label: "Turkish" },
  { value: "cs_cz", label: "Czech" },
  { value: "hu_hu", label: "Hungarian" },
  { value: "th_th", label: "Thai" },
  { value: "vn_vn", label: "Vietnamese" },
  { value: "id_id", label: "Indonesian" },
  { value: "ms_my", label: "Malay" },
] as const;

export function StringOverridesDialog({ open, project, onClose }: StringOverridesDialogProps) {
  const [selectedLayer, setSelectedLayer] = useState<string>("base");
  const [selectedLocale, setSelectedLocale] = useState<string>("default");
  const [entries, setEntries] = useState<OverrideEntry[]>([]);
  const [hasChanges, setHasChanges] = useState(false);
  const [errors, setErrors] = useState<Record<number, string>>({});

  const saveOverrides = useSaveStringOverrides();

  // Get the currently selected layer data
  const currentLayer = project?.layers.find((l) => l.name === selectedLayer);

  // Reset entries when dialog opens or layer/locale changes
  useEffect(() => {
    if (!currentLayer) {
      setEntries([]);
      return;
    }
    const localeOverrides = currentLayer.stringOverrides?.[selectedLocale] ?? {};
    const initial = Object.entries(localeOverrides).map(([key, value]) => ({ key, value }));
    setEntries(initial);
    setHasChanges(false);
    setErrors({});
  }, [currentLayer?.name, selectedLocale, open]);

  // Reset selected layer when dialog opens with a new project
  useEffect(() => {
    if (open && project?.layers.length) {
      setSelectedLayer(project.layers[0].name);
      setSelectedLocale("default");
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
    if (!project || !currentLayer || !validate(entries)) return;

    // Build the overrides map for the current locale
    const localeOverrides: Record<string, string> = {};
    for (const entry of entries) {
      const trimmedKey = entry.key.trim();
      if (trimmedKey) {
        localeOverrides[trimmedKey] = entry.value;
      }
    }

    // Preserve existing overrides for other locales
    const allOverrides: Record<string, Record<string, string>> = {
      ...currentLayer.stringOverrides,
    };

    // Update or remove the current locale's overrides
    if (Object.keys(localeOverrides).length > 0) {
      allOverrides[selectedLocale] = localeOverrides;
    } else {
      delete allOverrides[selectedLocale];
    }

    saveOverrides.mutate(
      {
        projectPath: project.path,
        layerName: selectedLayer,
        stringOverrides: allOverrides,
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
            {/* Locale selector */}
            <div className="mb-4">
              <Field.Root>
                <Field.Label>Locale</Field.Label>
                <div className="relative">
                  <LuGlobe className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-surface-400" />
                  <select
                    value={selectedLocale}
                    onChange={(e) => setSelectedLocale(e.target.value)}
                    className="w-full appearance-none rounded-lg border border-surface-600 bg-surface-700 px-9 py-2 text-sm text-surface-100 outline-none transition-colors hover:border-surface-500 focus:border-accent-500 focus:ring-2 focus:ring-accent-500/20"
                  >
                    {LOCALES.map((locale) => (
                      <option key={locale.value} value={locale.value}>
                        {locale.label}
                      </option>
                    ))}
                  </select>
                </div>
              </Field.Root>
            </div>

            <div className="mb-3 flex items-center justify-between">
              <p className="text-sm text-surface-400">
                {overrideCount === 0
                  ? "No string overrides configured for this locale."
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
                        type="text"
                        value={entry.key}
                        onChange={(e) => handleUpdateEntry(index, "key", e.target.value)}
                        placeholder="game_character_displayname_Ahri"
                      />
                      {errors[index] && (
                        <Field.Error>{errors[index]}</Field.Error>
                      )}
                    </Field.Root>

                    <Field.Root className="flex-1">
                      <Field.Control
                        type="text"
                        value={entry.value}
                        onChange={(e) => handleUpdateEntry(index, "value", e.target.value)}
                        placeholder="Fox Spirit"
                      />
                    </Field.Root>

                    <IconButton
                      icon={<LuTrash2 className="h-4 w-4" />}
                      variant="ghost"
                      size="sm"
                      onClick={() => handleRemoveEntry(index)}
                      className="mt-1"
                    />
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Footer */}
          <div className="flex items-center justify-end gap-3 border-t border-surface-600 px-6 py-4">
            <Button variant="ghost" onClick={handleClose}>
              Cancel
            </Button>
            <Button
              variant="filled"
              onClick={handleSave}
              disabled={!hasChanges || saveOverrides.isPending}
            >
              {saveOverrides.isPending ? "Saving..." : "Save Changes"}
            </Button>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

// Badge showing override count for a layer
function OverrideBadge({ layer }: { layer: WorkshopLayer }) {
  const totalCount = Object.values(layer.stringOverrides).reduce(
    (sum, localeOverrides) => sum + Object.keys(localeOverrides).length,
    0,
  );

  if (totalCount === 0) return null;

  return (
    <span className="ml-2 rounded-full bg-accent-500/20 px-2 py-0.5 text-xs font-medium text-accent-400">
      {totalCount}
    </span>
  );
}
