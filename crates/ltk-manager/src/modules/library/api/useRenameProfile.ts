import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type Profile } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { libraryKeys } from "./keys";

interface RenameProfileVariables {
  profileId: string;
  newName: string;
}

/**
 * Hook to rename a profile.
 */
export function useRenameProfile() {
  const queryClient = useQueryClient();

  return useMutation<Profile, AppError, RenameProfileVariables>({
    mutationFn: async ({ profileId, newName }) => {
      const result = await api.renameModProfile(profileId, newName);
      return unwrapForQuery(result);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: libraryKeys.profiles() });
      queryClient.invalidateQueries({ queryKey: libraryKeys.activeProfile() });
    },
  });
}
