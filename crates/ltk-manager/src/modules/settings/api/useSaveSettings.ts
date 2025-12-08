import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type Settings } from "@/lib/tauri";
import { mutationFn } from "@/utils/query";

import { settingsKeys } from "./keys";

/**
 * Hook to save app settings.
 */
export function useSaveSettings() {
  const queryClient = useQueryClient();

  return useMutation<void, AppError, Settings>({
    mutationFn: mutationFn(api.saveSettings),
    onSuccess: (_data, settings) => {
      // Update the settings cache with the new values
      queryClient.setQueryData(settingsKeys.settings(), settings);
      // Invalidate setup required query as it may have changed
      queryClient.invalidateQueries({ queryKey: settingsKeys.setupRequired() });
    },
  });
}
