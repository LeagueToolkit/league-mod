import { queryOptions, skipToken, useQuery } from "@tanstack/react-query";

import { api, type AppError, type ValidationResult } from "@/lib/tauri";
import { queryFnWithArgs } from "@/utils/query";

import { workshopKeys } from "./keys";

/**
 * Query options for validating a project before packing.
 */
export function validateProjectOptions(projectPath: string | undefined) {
  return queryOptions<ValidationResult, AppError>({
    queryKey: workshopKeys.validation(projectPath ?? ""),
    queryFn: projectPath ? queryFnWithArgs(api.validateProject, projectPath) : skipToken,
  });
}

/**
 * Hook to validate a project before packing.
 */
export function useValidateProject(projectPath: string, enabled = true) {
  return useQuery({
    ...validateProjectOptions(enabled ? projectPath : undefined),
  });
}
