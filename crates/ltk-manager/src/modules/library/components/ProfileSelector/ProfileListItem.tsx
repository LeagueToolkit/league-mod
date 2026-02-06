import { LuCheck, LuPencil, LuTrash2 } from "react-icons/lu";

import { Button, IconButton } from "@/components";
import type { Profile } from "@/lib/tauri";

import { ProfileEditForm } from "./ProfileEditForm";

interface ProfileListItemProps {
  profile: Profile;
  isActive: boolean;
  isEditing: boolean;
  editValue: string;
  onSwitch: (profileId: string) => void;
  onStartEdit: (profileId: string, currentName: string) => void;
  onEditChange: (value: string) => void;
  onEditSubmit: (profileId: string) => void;
  onEditCancel: () => void;
  onDeleteClick: (profile: Profile) => void;
  isSwitching: boolean;
  isRenaming: boolean;
  isDeleting: boolean;
}

export function ProfileListItem({
  profile,
  isActive,
  isEditing,
  editValue,
  onSwitch,
  onStartEdit,
  onEditChange,
  onEditSubmit,
  onEditCancel,
  onDeleteClick,
  isSwitching,
  isRenaming,
  isDeleting,
}: ProfileListItemProps) {
  const isDefaultProfile = profile.name === "Default";

  if (isEditing) {
    return (
      <ProfileEditForm
        profileId={profile.id}
        value={editValue}
        onChange={onEditChange}
        onSubmit={onEditSubmit}
        onCancel={onEditCancel}
        isSubmitting={isRenaming}
      />
    );
  }

  return (
    <div className="flex items-center gap-1">
      <Button
        variant="ghost"
        size="sm"
        onClick={() => onSwitch(profile.id)}
        disabled={isSwitching || isActive}
        className="flex-1 justify-between"
        right={isActive ? <LuCheck className="h-4 w-4 text-brand-500" /> : undefined}
      >
        {profile.name}
      </Button>

      {/* Edit and Delete buttons (hide for Default profile) */}
      {!isDefaultProfile && (
        <>
          <IconButton
            icon={<LuPencil className="h-3.5 w-3.5" />}
            variant="ghost"
            size="xs"
            onClick={() => onStartEdit(profile.id, profile.name)}
            title="Rename profile"
          />
          <IconButton
            icon={<LuTrash2 className="h-3.5 w-3.5" />}
            variant="ghost"
            size="xs"
            onClick={() => onDeleteClick(profile)}
            disabled={isDeleting || isActive}
            className="hover:text-red-400"
            title="Delete profile"
          />
        </>
      )}
    </div>
  );
}
