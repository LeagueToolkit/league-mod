import { create } from "zustand";

export type Theme = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

interface ThemeState {
  theme: Theme;
  resolvedTheme: ResolvedTheme;
  setTheme: (theme: Theme) => void;
}

export const useThemeStore = create<ThemeState>((set) => ({
  theme: "dark",
  resolvedTheme: "dark",
  setTheme: (theme: Theme) => {
    // Theme switching is disabled for now - will be improved later
    set({ theme, resolvedTheme: theme === "light" ? "light" : "dark" });
  },
}));

// Theme initialization is disabled for now - always use dark theme
export function initializeTheme(_theme: Theme) {
  // No-op: theme switching disabled, keeping dark theme
  return () => {};
}
