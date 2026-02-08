import { Button } from "@/components";
import type { Settings } from "@/lib/tauri";

interface ThemePickerProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function ThemePicker({ settings, onSave }: ThemePickerProps) {
  return (
    <div className="space-y-3">
      <span className="block text-sm font-medium text-surface-400">Theme</span>
      <div className="flex gap-2">
        {(["system", "dark", "light"] as const).map((theme) => (
          <Button
            key={theme}
            variant={settings.theme === theme ? "filled" : "default"}
            size="sm"
            onClick={() => onSave({ ...settings, theme })}
            className="capitalize"
          >
            {theme}
          </Button>
        ))}
      </div>
    </div>
  );
}
