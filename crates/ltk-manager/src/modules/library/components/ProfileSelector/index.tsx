import { useEffect, useRef, useState } from "react";

import { useToast } from "@/components";
import type { Profile } from "@/lib/tauri";
import {
  useActiveProfile,
  useCreateProfile,
  useDeleteProfile,
  useProfiles,
  useRenameProfile,
  useSwitchProfile,
} from "@/modules/library/api";

import { ProfileCreateForm } from "./ProfileCreateForm";
import { ProfileDeleteDialog } from "./ProfileDeleteDialog";
import { ProfileDropdownTrigger } from "./ProfileDropdownTrigger";
import { ProfileListItem } from "./ProfileListItem";

export function ProfileSelector() {
  const { data: profiles = [] } = useProfiles();
  const { data: activeProfile } = useActiveProfile();
  const switchProfile = useSwitchProfile();
  const createProfile = useCreateProfile();
  const deleteProfile = useDeleteProfile();
  const renameProfile = useRenameProfile();
  const toast = useToast();

  const [isOpen, setIsOpen] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [profileToDelete, setProfileToDelete] = useState<Profile | null>(null);

  const dropdownRef = useRef<HTMLDivElement>(null);

  // Close dropdown when clicking outside
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
        setIsCreating(false);
        setEditingId(null);
      }
    }

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [isOpen]);

  const handleSwitch = async (profileId: string) => {
    try {
      await switchProfile.mutateAsync(profileId);
      setIsOpen(false);
    } catch (error: any) {
      toast.error("Failed to switch profile", error.message);
    }
  };

  const handleCreate = async () => {
    const trimmedName = newProfileName.trim();
    if (!trimmedName) return;

    try {
      await createProfile.mutateAsync(trimmedName);
      setNewProfileName("");
      setIsCreating(false);
      toast.success("Profile created", `Profile "${trimmedName}" has been created.`);
    } catch (error: any) {
      toast.error("Failed to create profile", error.message || String(error));
    }
  };

  const handleDeleteClick = (profile: Profile) => {
    setProfileToDelete(profile);
  };

  const handleDeleteConfirm = async () => {
    if (!profileToDelete) return;

    try {
      await deleteProfile.mutateAsync(profileToDelete.id);
      setProfileToDelete(null);
      toast.success("Profile deleted");
    } catch (error: any) {
      toast.error("Failed to delete profile", error.message);
    }
  };

  const handleDeleteCancel = () => {
    setProfileToDelete(null);
  };

  const handleRename = async (profileId: string) => {
    const trimmedName = editingName.trim();
    if (!trimmedName) return;

    try {
      await renameProfile.mutateAsync({ profileId, newName: trimmedName });
      setEditingId(null);
      setEditingName("");
      toast.success("Profile renamed");
    } catch (error: any) {
      toast.error("Failed to rename profile", error.message);
    }
  };

  const startEditing = (profileId: string, currentName: string) => {
    setEditingId(profileId);
    setEditingName(currentName);
  };

  const cancelEditing = () => {
    setEditingId(null);
    setEditingName("");
  };

  const cancelCreating = () => {
    setIsCreating(false);
    setNewProfileName("");
  };

  return (
    <div className="relative" ref={dropdownRef}>
      <ProfileDropdownTrigger
        activeProfileName={activeProfile?.name || "Default"}
        isOpen={isOpen}
        onClick={() => setIsOpen(!isOpen)}
      />

      {/* Dropdown Menu */}
      {isOpen && (
        <div className="absolute top-full left-0 z-50 mt-1 w-64 rounded-lg border border-surface-600 bg-surface-800 shadow-xl">
          <div className="max-h-[400px] overflow-y-auto p-1">
            {/* Profile List */}
            {profiles.map((profile) => (
              <ProfileListItem
                key={profile.id}
                profile={profile}
                isActive={profile.id === activeProfile?.id}
                isEditing={editingId === profile.id}
                editValue={editingName}
                onSwitch={handleSwitch}
                onStartEdit={startEditing}
                onEditChange={setEditingName}
                onEditSubmit={handleRename}
                onEditCancel={cancelEditing}
                onDeleteClick={handleDeleteClick}
                isSwitching={switchProfile.isPending}
                isRenaming={renameProfile.isPending}
                isDeleting={deleteProfile.isPending}
              />
            ))}

            {/* Create New Profile */}
            <div className="mt-1 border-t border-surface-700 pt-1">
              <ProfileCreateForm
                isCreating={isCreating}
                value={newProfileName}
                onChange={setNewProfileName}
                onSubmit={handleCreate}
                onCancel={cancelCreating}
                onStartCreating={() => setIsCreating(true)}
                isSubmitting={createProfile.isPending}
              />
            </div>
          </div>
        </div>
      )}

      {/* Delete Confirmation Dialog */}
      <ProfileDeleteDialog
        open={!!profileToDelete}
        profile={profileToDelete}
        onClose={handleDeleteCancel}
        onConfirm={handleDeleteConfirm}
        isPending={deleteProfile.isPending}
      />
    </div>
  );
}
