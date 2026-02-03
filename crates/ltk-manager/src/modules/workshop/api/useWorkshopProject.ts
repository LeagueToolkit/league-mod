import { queryOptions, skipToken, useQuery } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { queryFnWithArgs } from "@/utils/query";

import { workshopKeys } from "./keys";

/**
 * Query options for fetching a single workshop project.
 */
export function workshopProjectOptions(projectPath: string | undefined) {
  return queryOptions<WorkshopProject, AppError>({
    queryKey: workshopKeys.project(projectPath ?? ""),
    queryFn: projectPath ? queryFnWithArgs(api.getWorkshopProject, projectPath) : skipToken,
  });
}

/**
 * Hook to fetch a single workshop project by path.
 */
export function useWorkshopProject(projectPath: string) {
  return useQuery(workshopProjectOptions(projectPath));
}
