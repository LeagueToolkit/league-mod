import { LuTriangleAlert } from "react-icons/lu";

import { Button, Dialog } from "@/components";
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
        <Dialog.Backdrop />
        <Dialog.Overlay>
          <Dialog.Header>
            <Dialog.Title>Delete Profile</Dialog.Title>
            <Dialog.Close />
          </Dialog.Header>

          <Dialog.Body>
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
          </Dialog.Body>

          <Dialog.Footer>
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
          </Dialog.Footer>
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
