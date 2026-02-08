import { open } from "@tauri-apps/plugin-dialog";
import { LuFolderOpen } from "react-icons/lu";

import { Field, IconButton } from "@/components";
import type { Settings } from "@/lib/tauri";

interface ModStorageSectionProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function ModStorageSection({ settings, onSave }: ModStorageSectionProps) {
  async function handleBrowse() {
    try {
      const selected = await open({
        directory: true,
        title: "Select Mod Storage Location",
      });

      if (selected) {
        onSave({ ...settings, modStoragePath: selected as string });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  return (
    <section>
      <h3 className="mb-4 text-lg font-medium text-surface-100">Mod Storage</h3>
      <div className="space-y-3">
        <span className="block text-sm font-medium text-surface-400">Storage Location</span>
        <div className="flex gap-2">
          <Field.Control
            type="text"
            value={settings.modStoragePath || ""}
            readOnly
            placeholder="Default (app data directory)"
            className="flex-1"
          />
          <IconButton
            icon={<LuFolderOpen className="h-5 w-5" />}
            variant="outline"
            size="lg"
            onClick={handleBrowse}
          />
        </div>
        <p className="text-sm text-surface-500">
          Choose where your installed mods will be stored. Leave empty to use the default location.
        </p>
      </div>
    </section>
  );
}
