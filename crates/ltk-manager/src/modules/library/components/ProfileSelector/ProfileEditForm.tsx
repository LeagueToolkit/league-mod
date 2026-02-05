import { useEffect, useRef } from "react";
import { LuCheck, LuX } from "react-icons/lu";

import { IconButton } from "@/components/Button";
import { Field } from "@/components/FormField";

interface ProfileEditFormProps {
  profileId: string;
  value: string;
  onChange: (value: string) => void;
  onSubmit: (profileId: string) => void;
  onCancel: () => void;
  isSubmitting: boolean;
}

export function ProfileEditForm({
  profileId,
  value,
  onChange,
  onSubmit,
  onCancel,
  isSubmitting,
}: ProfileEditFormProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  // Focus input when mounted
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      onSubmit(profileId);
    } else if (e.key === "Escape") {
      onCancel();
    }
  };

  return (
    <div className="flex items-center gap-1 p-1">
      <Field.Control
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={handleKeyDown}
        className="h-7 flex-1 px-2 py-1 text-sm"
        placeholder="Profile name..."
      />
      <IconButton
        icon={<LuCheck className="h-4 w-4" />}
        variant="ghost"
        size="xs"
        onClick={() => onSubmit(profileId)}
        disabled={!value.trim() || isSubmitting}
        className="text-green-400 hover:text-green-300"
      />
      <IconButton icon={<LuX className="h-4 w-4" />} variant="ghost" size="xs" onClick={onCancel} />
    </div>
  );
}
