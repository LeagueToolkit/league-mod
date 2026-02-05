import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type Profile } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { libraryKeys } from "./keys";

/**
 * Hook to create a new profile.
 */
export function useCreateProfile() {
  const queryClient = useQueryClient();

  return useMutation<Profile, AppError, string>({
    mutationFn: async (name) => {
      const result = await api.createModProfile(name);
      return unwrapForQuery(result);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: libraryKeys.profiles() });
    },
  });
}
