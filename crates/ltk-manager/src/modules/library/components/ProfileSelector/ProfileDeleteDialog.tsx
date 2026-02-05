import { Dialog } from "@base-ui-components/react";
import { LuTriangleAlert, LuX } from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";
import type { Profile } from "@/lib/tauri";

interface ProfileDeleteDialogProps {
  open: boolean;
  profile: Profile | null;
  onClose: () => void;
  onConfirm: () => void;
  isPending?: boolean;
}

export function ProfileDeleteDialog({
  open,
  profile,
  onClose,
  onConfirm,
  isPending,
}: ProfileDeleteDialogProps) {
  if (!profile) return null;

  return (
    <Dialog.Root open={open} onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm" />
        <Dialog.Popup className="fixed top-1/2 left-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-xl border border-surface-600 bg-surface-800 shadow-2xl">
          <div className="flex items-center justify-between border-b border-surface-600 px-6 py-4">
            <Dialog.Title className="text-lg font-semibold text-surface-100">
              Delete Profile
            </Dialog.Title>
            <IconButton
              icon={<LuX className="h-5 w-5" />}
              variant="ghost"
              size="sm"
              onClick={onClose}
            />
          </div>

          <div className="px-6 py-4">
            <div className="flex items-start gap-3 rounded-lg border border-red-500/30 bg-red-500/10 p-4">
              <LuTriangleAlert className="mt-0.5 h-5 w-5 shrink-0 text-red-400" />
              <div>
                <h3 className="font-medium text-red-300">
                  Are you sure you want to delete &ldquo;{profile.name}&rdquo;?
                </h3>
                <p className="mt-1 text-sm text-surface-400">
                  This will permanently delete the profile and all its configuration. Any enabled
                  mods in this profile will remain installed but will need to be re-enabled in
                  another profile.
                </p>
                <p className="mt-2 text-xs text-surface-500">This action cannot be undone.</p>
              </div>
            </div>
          </div>

          <div className="flex justify-end gap-3 border-t border-surface-600 px-6 py-4">
            <Button variant="ghost" onClick={onClose} disabled={isPending}>
              Cancel
            </Button>
            <Button
              variant="filled"
              onClick={onConfirm}
              loading={isPending}
              className="bg-red-600 hover:bg-red-500"
            >
              Delete Profile
            </Button>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
