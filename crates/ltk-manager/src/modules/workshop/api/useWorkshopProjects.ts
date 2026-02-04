import { queryOptions, useQuery } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { workshopKeys } from "./keys";

/**
 * Query options for fetching all workshop projects.
 */
export function workshopProjectsOptions() {
  return queryOptions<WorkshopProject[], AppError>({
    queryKey: workshopKeys.projects(),
    queryFn: queryFn(api.getWorkshopProjects),
  });
}

/**
 * Hook to fetch all workshop projects.
 */
export function useWorkshopProjects() {
  return useQuery(workshopProjectsOptions());
}
