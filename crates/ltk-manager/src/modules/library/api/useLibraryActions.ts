import { open } from "@tauri-apps/plugin-dialog";

import { useToast } from "@/components";
import { api, unwrap } from "@/lib/tauri";

import { useInstallMod } from "./useInstallMod";
import { useReorderMods } from "./useReorderMods";
import { useToggleMod } from "./useToggleMod";
import { useUninstallMod } from "./useUninstallMod";

export function useLibraryActions() {
  const installMod = useInstallMod();
  const toggleMod = useToggleMod();
  const uninstallMod = useUninstallMod();
  const reorderMods = useReorderMods();
  const toast = useToast();

  async function handleInstallMod() {
    const file = await open({
      multiple: false,
      filters: [{ name: "Mod Package", extensions: ["modpkg", "fantome"] }],
    });

    if (file) {
      installMod.mutate(file, {
        onError: (error) => {
          console.error("Failed to install mod:", error.message);
        },
      });
    }
  }

  function handleToggleMod(modId: string, enabled: boolean) {
    toggleMod.mutate(
      { modId, enabled },
      {
        onError: (error) => {
          console.error("Failed to toggle mod:", error.message);
        },
      },
    );
  }

  function handleUninstallMod(modId: string) {
    uninstallMod.mutate(modId, {
      onError: (error) => {
        console.error("Failed to uninstall mod:", error.message);
      },
    });
  }

  function handleReorder(modIds: string[]) {
    reorderMods.mutate(modIds);
  }

  async function handleOpenStorageDirectory() {
    try {
      const result = await api.getStorageDirectory();
      const path = unwrap(result);
      await api.revealInExplorer(path);
    } catch (error: unknown) {
      toast.error(
        "Failed to open directory",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  return {
    installMod,
    handleInstallMod,
    handleToggleMod,
    handleUninstallMod,
    handleReorder,
    handleOpenStorageDirectory,
  };
}
