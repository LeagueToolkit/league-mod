export const libraryKeys = {
  all: ["library"] as const,
  mods: () => [...libraryKeys.all, "mods"] as const,
  mod: (id: string) => [...libraryKeys.mods(), id] as const,
  thumbnail: (modId: string, path?: string) => [...libraryKeys.mod(modId), "thumbnail", path] as const,
};
