export const settingsKeys = {
  all: ["settings"] as const,
  settings: () => [...settingsKeys.all, "current"] as const,
  setupRequired: () => [...settingsKeys.all, "setupRequired"] as const,
  appInfo: () => [...settingsKeys.all, "appInfo"] as const,
};
