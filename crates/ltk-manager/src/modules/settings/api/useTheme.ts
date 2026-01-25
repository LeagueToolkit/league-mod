import { useEffect } from "react";

import { useSettings } from "./useSettings";

// Accent color preset hues
const ACCENT_PRESETS: Record<string, number> = {
  blue: 207,
  purple: 271,
  green: 122,
  orange: 36,
  pink: 340,
  red: 4,
  teal: 174,
};

/**
 * Hook to apply theme and accent color to the document.
 * Should be used at the app root level.
 */
export function useTheme() {
  const { data: settings } = useSettings();

  useEffect(() => {
    if (!settings) return;

    const root = document.documentElement;

    // Handle theme preference
    const applyTheme = (isDark: boolean) => {
      if (isDark) {
        root.classList.remove("light");
        root.classList.add("dark");
      } else {
        root.classList.remove("dark");
        root.classList.add("light");
      }
    };

    if (settings.theme === "system") {
      // Listen to system preference
      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
      applyTheme(mediaQuery.matches);

      const handleChange = (e: MediaQueryListEvent) => {
        applyTheme(e.matches);
      };

      mediaQuery.addEventListener("change", handleChange);
      return () => mediaQuery.removeEventListener("change", handleChange);
    } else {
      applyTheme(settings.theme === "dark");
    }
  }, [settings?.theme]);

  useEffect(() => {
    if (!settings) return;

    const root = document.documentElement;

    // Apply accent color
    let hue: number;

    if (settings.accentColor?.customHue != null) {
      hue = settings.accentColor.customHue;
    } else if (settings.accentColor?.preset && ACCENT_PRESETS[settings.accentColor.preset]) {
      hue = ACCENT_PRESETS[settings.accentColor.preset];
    } else {
      hue = ACCENT_PRESETS.blue; // Default to blue
    }

    root.style.setProperty("--accent-hue", String(hue));
  }, [settings?.accentColor]);
}

export { ACCENT_PRESETS };
