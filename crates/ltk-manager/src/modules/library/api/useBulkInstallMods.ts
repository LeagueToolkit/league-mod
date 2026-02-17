import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type BulkInstallResult, type InstalledMod } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { libraryKeys } from "./keys";

export function useBulkInstallMods() {
  const queryClient = useQueryClient();

  return useMutation<BulkInstallResult, AppError, string[]>({
    mutationFn: async (filePaths) => {
      const result = await api.installMods(filePaths);
      return unwrapForQuery(result);
    },
    onSuccess: (result) => {
      if (result.installed.length > 0) {
        queryClient.setQueryData<InstalledMod[]>(libraryKeys.mods(), (old) =>
          old ? [...old, ...result.installed] : result.installed,
        );
      }
    },
  });
}
