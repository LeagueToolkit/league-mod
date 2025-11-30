import { useQuery } from "@tanstack/react-query";
import { api, type InstalledMod, type AppError } from "@/lib/tauri";
import { queryFn } from "@/utils/query";
import { libraryKeys } from "./keys";

/**
 * Hook to fetch all installed mods.
 */
export function useInstalledMods() {
  return useQuery<InstalledMod[], AppError>({
    queryKey: libraryKeys.mods(),
    queryFn: queryFn(api.getInstalledMods),
  });
}

