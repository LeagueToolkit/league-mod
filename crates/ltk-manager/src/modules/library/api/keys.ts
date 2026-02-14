export const libraryKeys = {
  all: ["library"] as const,
  mods: () => [...libraryKeys.all, "mods"] as const,
  mod: (id: string) => [...libraryKeys.mods(), id] as const,
  thumbnail: (modId: string) => [...libraryKeys.mod(modId), "thumbnail"] as const,
  profiles: () => [...libraryKeys.all, "profiles"] as const,
  activeProfile: () => [...libraryKeys.profiles(), "active"] as const,
};
