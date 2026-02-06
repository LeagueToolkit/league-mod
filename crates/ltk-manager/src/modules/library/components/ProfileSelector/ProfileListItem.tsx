import { useEffect, useRef, useState } from "react";
import { LuCheck, LuPencil, LuTrash2, LuX } from "react-icons/lu";

import { Button, Field, IconButton, useToast } from "@/components";
import type { Profile } from "@/lib/tauri";
import { useRenameProfile } from "@/modules/library/api";

interface ProfileListItemProps {
  profile: Profile;
  isActive: boolean;
  onSwitch: (profileId: string) => void;
  onDeleteClick: (profile: Profile) => void;
  isSwitching: boolean;
}

export function ProfileListItem({
  profile,
  isActive,
  onSwitch,
  onDeleteClick,
  isSwitching,
}: ProfileListItemProps) {
  const renameProfile = useRenameProfile();
  const toast = useToast();

  const [isEditing, setIsEditing] = useState(false);
  const [editName, setEditName] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  const isDefaultProfile = profile.name === "Default";

  useEffect(() => {
    if (isEditing) {
      inputRef.current?.focus();
    }
  }, [isEditing]);

  const startEditing = () => {
    setIsEditing(true);
    setEditName(profile.name);
  };

  const cancelEditing = () => {
    setIsEditing(false);
    setEditName("");
  };

  const handleRename = async () => {
    const trimmedName = editName.trim();
    if (!trimmedName) return;

    try {
      await renameProfile.mutateAsync({ profileId: profile.id, newName: trimmedName });
      setIsEditing(false);
      setEditName("");
      toast.success("Profile renamed");
    } catch (error: unknown) {
      toast.error(
        "Failed to rename profile",
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      handleRename();
    } else if (e.key === "Escape") {
      cancelEditing();
    }
  };

  if (isEditing) {
    return (
      <div className="flex items-center gap-1 p-1">
        <Field.Control
          ref={inputRef}
          type="text"
          value={editName}
          onChange={(e) => setEditName(e.target.value)}
          onKeyDown={handleKeyDown}
          className="h-7 flex-1 px-2 py-1 text-sm"
          placeholder="Profile name..."
        />
        <IconButton
          icon={<LuCheck className="h-4 w-4" />}
          variant="ghost"
          size="xs"
          onClick={handleRename}
          disabled={!editName.trim() || renameProfile.isPending}
          className="text-green-400 hover:text-green-300"
        />
        <IconButton
          icon={<LuX className="h-4 w-4" />}
          variant="ghost"
          size="xs"
          onClick={cancelEditing}
        />
      </div>
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

      {!isDefaultProfile && (
        <>
          <IconButton
            icon={<LuPencil className="h-3.5 w-3.5" />}
            variant="ghost"
            size="xs"
            onClick={startEditing}
            title="Rename profile"
          />
          <IconButton
            icon={<LuTrash2 className="h-3.5 w-3.5" />}
            variant="ghost"
            size="xs"
            onClick={() => onDeleteClick(profile)}
            disabled={isActive}
            className="hover:text-red-400"
            title="Delete profile"
          />
        </>
      )}
    </div>
  );
}
