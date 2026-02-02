import { skipToken, useQuery } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api, type AppError } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { libraryKeys } from "./keys";

/**
 * Hook to fetch and cache mod thumbnail absolute path, returning a Tauri asset URL.
 */
export function useModThumbnail(modId: string, thumbnailPath?: string) {
  return useQuery<string, AppError, string>({
    queryKey: libraryKeys.thumbnail(modId, thumbnailPath),
    queryFn: thumbnailPath ? queryFn(() => api.getModThumbnail(thumbnailPath)) : skipToken,
    select: (path) => (path ? convertFileSrc(path) : ""),
    staleTime: Infinity, // Thumbnails don't change often
  });
}
