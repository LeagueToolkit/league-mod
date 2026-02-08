import { open } from "@tauri-apps/plugin-dialog";
import { LuFolderOpen } from "react-icons/lu";

import { Field, IconButton } from "@/components";
import type { Settings } from "@/lib/tauri";

interface WorkshopSectionProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function WorkshopSection({ settings, onSave }: WorkshopSectionProps) {
  async function handleBrowse() {
    try {
      const selected = await open({
        directory: true,
        title: "Select Workshop Directory",
      });

      if (selected) {
        onSave({ ...settings, workshopPath: selected as string });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  return (
    <section>
      <h3 className="mb-4 text-lg font-medium text-surface-100">Workshop</h3>
      <div className="space-y-3">
        <span className="block text-sm font-medium text-surface-400">Workshop Directory</span>
        <div className="flex gap-2">
          <Field.Control
            type="text"
            value={settings.workshopPath || ""}
            readOnly
            placeholder="Not configured"
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
          Choose where your mod projects will be stored for the Creator Workshop. This directory
          will contain all your project folders.
        </p>
      </div>
    </section>
  );
}
