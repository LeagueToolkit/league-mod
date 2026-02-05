import { queryOptions, useQuery } from "@tanstack/react-query";

import { api, type AppError, type InstalledMod, type Profile } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { libraryKeys } from "./keys";

// ============================================================================
// Profiles
// ============================================================================

/**
 * Query options for fetching all profiles.
 */
export function profilesQueryOptions() {
  return queryOptions<Profile[], AppError>({
    queryKey: libraryKeys.profiles(),
    queryFn: queryFn(api.listModProfiles),
  });
}

/**
 * Hook to fetch all profiles.
 */
export function useProfiles() {
  return useQuery(profilesQueryOptions());
}

/**
 * Query options for fetching the currently active profile.
 */
export function activeProfileQueryOptions() {
  return queryOptions<Profile, AppError>({
    queryKey: libraryKeys.activeProfile(),
    queryFn: queryFn(api.getActiveModProfile),
  });
}

/**
 * Hook to fetch the currently active profile.
 */
export function useActiveProfile() {
  return useQuery(activeProfileQueryOptions());
}

// ============================================================================
// Mods
// ============================================================================

/**
 * Query options for fetching all installed mods.
 */
export function installedModsQueryOptions() {
  return queryOptions<InstalledMod[], AppError>({
    queryKey: libraryKeys.mods(),
    queryFn: queryFn(api.getInstalledMods),
  });
}

/**
 * Hook to fetch all installed mods.
 */
export function useInstalledMods() {
  return useQuery(installedModsQueryOptions());
}
