import { open } from "@tauri-apps/plugin-dialog";
import { LuImage, LuX } from "react-icons/lu";

import { Field, IconButton } from "@/components";
import type { Settings } from "@/lib/tauri";

import { useDebouncedSlider } from "./useDebouncedSlider";

interface BackdropImagePickerProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function BackdropImagePicker({ settings, onSave }: BackdropImagePickerProps) {
  const [localBlur, handleBlurChange] = useDebouncedSlider(settings.backdropBlur ?? 40, (blur) => {
    onSave({ ...settings, backdropBlur: blur });
  });

  async function handleBrowse() {
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

  function handleClear() {
    onSave({ ...settings, backdropImage: null });
  }

  return (
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
          onClick={handleBrowse}
        />
        {settings.backdropImage && (
          <IconButton
            icon={<LuX className="h-5 w-5" />}
            variant="outline"
            size="lg"
            onClick={handleClear}
          />
        )}
      </div>
      <p className="text-sm text-surface-500">
        Set a background image for the app. The UI will render with a frosted glass effect over the
        image.
      </p>

      {/* Blur slider — only visible when a backdrop image is set */}
      {settings.backdropImage && (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-xs text-surface-500">Blur Amount</span>
            <span className="text-xs text-surface-400">{localBlur}px</span>
          </div>
          <input
            type="range"
            min="0"
            max="100"
            value={localBlur}
            onChange={(e) => handleBlurChange(Number(e.target.value))}
            className="h-2 w-full cursor-pointer appearance-none rounded-full bg-surface-600"
          />
        </div>
      )}
    </div>
  );
}
