import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { libraryKeys } from "./keys";

/**
 * Hook to delete a profile.
 */
export function useDeleteProfile() {
  const queryClient = useQueryClient();

  return useMutation<void, AppError, string>({
    mutationFn: async (profileId) => {
      const result = await api.deleteModProfile(profileId);
      return unwrapForQuery(result);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: libraryKeys.profiles() });
    },
  });
}
