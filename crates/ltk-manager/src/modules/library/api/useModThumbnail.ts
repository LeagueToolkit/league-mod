import { useQuery } from "@tanstack/react-query";

import { api, type AppError } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { libraryKeys } from "./keys";

/**
 * Hook to fetch a mod thumbnail as a base64 data URL, loaded on-the-fly from the archive.
 * Returns `null` if the mod has no thumbnail.
 */
export function useModThumbnail(modId: string) {
  return useQuery<string | null, AppError>({
    queryKey: libraryKeys.thumbnail(modId),
    queryFn: queryFn(() => api.getModThumbnail(modId)),
    staleTime: Infinity,
  });
}
