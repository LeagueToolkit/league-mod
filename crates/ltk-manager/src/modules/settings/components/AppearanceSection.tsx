import { open } from "@tauri-apps/plugin-dialog";
import { LuCircleCheck, LuImage, LuX } from "react-icons/lu";

import { Button, Field, IconButton } from "@/components";
import type { Settings } from "@/lib/tauri";

import { ACCENT_PRESETS } from "../api";

const ACCENT_PRESET_DISPLAY: { key: string; label: string; color: string }[] = [
  { key: "blue", label: "Blue", color: "hsl(207, 100%, 50%)" },
  { key: "purple", label: "Purple", color: "hsl(271, 100%, 50%)" },
  { key: "green", label: "Green", color: "hsl(122, 100%, 35%)" },
  { key: "orange", label: "Orange", color: "hsl(36, 100%, 50%)" },
  { key: "pink", label: "Pink", color: "hsl(340, 100%, 50%)" },
  { key: "red", label: "Red", color: "hsl(4, 100%, 50%)" },
  { key: "teal", label: "Teal", color: "hsl(174, 100%, 35%)" },
];

interface AppearanceSectionProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function AppearanceSection({ settings, onSave }: AppearanceSectionProps) {
  const isCustomHue = settings.accentColor?.customHue != null;
  const currentHue = isCustomHue
    ? settings.accentColor.customHue!
    : settings.accentColor?.preset
      ? (ACCENT_PRESETS[settings.accentColor.preset] ?? 207)
      : 207;

  function handleAccentPresetClick(preset: string) {
    onSave({
      ...settings,
      accentColor: { preset, customHue: null },
    });
  }

  function handleCustomHueChange(hue: number) {
    onSave({
      ...settings,
      accentColor: { preset: null, customHue: hue },
    });
  }

  async function handleBrowseBackdropImage() {
    try {
      const selected = await open({
        title: "Select Background Image",
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif"] }],
      });

      if (selected) {
        onSave({ ...settings, backdropImage: selected as string });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  function handleClearBackdropImage() {
    onSave({ ...settings, backdropImage: null });
  }

  return (
    <section>
      <h3 className="mb-4 text-lg font-medium text-surface-100">Appearance</h3>

      {/* Theme */}
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

      {/* Accent Color */}
      <div className="mt-6 space-y-3">
        <span className="block text-sm font-medium text-surface-400">Accent Color</span>

        {/* Preset Colors */}
        <div className="flex flex-wrap gap-2">
          {ACCENT_PRESET_DISPLAY.map(({ key, label, color }) => (
            <button
              key={key}
              onClick={() => handleAccentPresetClick(key)}
              className={`group relative h-8 w-8 rounded-full transition-transform hover:scale-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-500 ${
                settings.accentColor?.preset === key && !isCustomHue
                  ? "ring-2 ring-surface-100 ring-offset-2 ring-offset-surface-900"
                  : ""
              }`}
              style={{ backgroundColor: color }}
              title={label}
            >
              {settings.accentColor?.preset === key && !isCustomHue && (
                <span className="absolute inset-0 flex items-center justify-center">
                  <LuCircleCheck className="h-4 w-4 text-white drop-shadow-md" />
                </span>
              )}
            </button>
          ))}
        </div>

        {/* Custom Color Slider */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-xs text-surface-500">Custom Color</span>
            {isCustomHue && (
              <span className="text-xs text-surface-400">Hue: {Math.round(currentHue)}°</span>
            )}
          </div>
          <div className="relative">
            <input
              type="range"
              min="0"
              max="360"
              value={currentHue}
              onChange={(e) => handleCustomHueChange(Number(e.target.value))}
              className="h-3 w-full cursor-pointer appearance-none rounded-full"
              style={{
                background: `linear-gradient(to right,
                  hsl(0, 100%, 50%),
                  hsl(60, 100%, 50%),
                  hsl(120, 100%, 50%),
                  hsl(180, 100%, 50%),
                  hsl(240, 100%, 50%),
                  hsl(300, 100%, 50%),
                  hsl(360, 100%, 50%)
                )`,
              }}
            />
            {/* Custom thumb indicator */}
            <div
              className="pointer-events-none absolute top-1/2 h-5 w-5 -translate-y-1/2 rounded-full border-2 border-white shadow-md"
              style={{
                left: `calc(${(currentHue / 360) * 100}% - 10px)`,
                backgroundColor: `hsl(${currentHue}, 100%, 50%)`,
              }}
            />
          </div>

          {/* Preview */}
          <div className="flex items-center gap-3">
            <div
              className="h-6 w-6 rounded"
              style={{ backgroundColor: `hsl(${currentHue}, 100%, 50%)` }}
            />
            <span className="text-sm text-surface-400">Preview</span>
          </div>
        </div>
      </div>

      {/* Background Image */}
      <div className="mt-6 space-y-3">
        <span className="block text-sm font-medium text-surface-400">Background Image</span>
        <div className="flex gap-2">
          <Field.Control
            type="text"
            value={settings.backdropImage || ""}
            readOnly
            placeholder="No image selected"
            className="flex-1"
          />
          <IconButton
            icon={<LuImage className="h-5 w-5" />}
            variant="outline"
            size="lg"
            onClick={handleBrowseBackdropImage}
          />
          {settings.backdropImage && (
            <IconButton
              icon={<LuX className="h-5 w-5" />}
              variant="outline"
              size="lg"
              onClick={handleClearBackdropImage}
            />
          )}
        </div>
        <p className="text-sm text-surface-500">
          Set a background image for the app. The UI will render with a frosted glass effect over
          the image.
        </p>

        {/* Blur slider — only visible when a backdrop image is set */}
        {settings.backdropImage && (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-xs text-surface-500">Blur Amount</span>
              <span className="text-xs text-surface-400">{settings.backdropBlur ?? 40}px</span>
            </div>
            <input
              type="range"
              min="0"
              max="100"
              value={settings.backdropBlur ?? 40}
              onChange={(e) => onSave({ ...settings, backdropBlur: Number(e.target.value) })}
              className="h-2 w-full cursor-pointer appearance-none rounded-full bg-surface-600"
            />
          </div>
        )}
      </div>
    </section>
  );
}
