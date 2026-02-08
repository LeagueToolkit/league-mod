import type { Settings } from "@/lib/tauri";

import { AccentColorPicker } from "./AccentColorPicker";
import { BackdropImagePicker } from "./BackdropImagePicker";
import { ThemePicker } from "./ThemePicker";

interface AppearanceSectionProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function AppearanceSection({ settings, onSave }: AppearanceSectionProps) {
  return (
    <section>
      <h3 className="mb-4 text-lg font-medium text-surface-100">Appearance</h3>
      <ThemePicker settings={settings} onSave={onSave} />
      <AccentColorPicker settings={settings} onSave={onSave} />
      <BackdropImagePicker settings={settings} onSave={onSave} />
    </section>
  );
}
