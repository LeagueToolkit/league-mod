import { useEffect, useRef } from "react";
import { LuCheck, LuPlus, LuX } from "react-icons/lu";

import { Button, IconButton } from "@/components/Button";
import { Field } from "@/components/FormField";

interface ProfileCreateFormProps {
  isCreating: boolean;
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onCancel: () => void;
  onStartCreating: () => void;
  isSubmitting: boolean;
}

export function ProfileCreateForm({
  isCreating,
  value,
  onChange,
  onSubmit,
  onCancel,
  onStartCreating,
  isSubmitting,
}: ProfileCreateFormProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  // Focus input when entering create mode
  useEffect(() => {
    if (isCreating) {
      inputRef.current?.focus();
    }
  }, [isCreating]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      onSubmit();
    } else if (e.key === "Escape") {
      onCancel();
    }
  };

  if (!isCreating) {
    return (
      <Button
        variant="ghost"
        size="sm"
        onClick={onStartCreating}
        left={<LuPlus className="h-4 w-4" />}
        className="w-full justify-start"
      >
        New Profile
      </Button>
    );
  }

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
        onClick={onSubmit}
        disabled={!value.trim() || isSubmitting}
        loading={isSubmitting}
        className="text-green-400 hover:text-green-300"
      />
      <IconButton icon={<LuX className="h-4 w-4" />} variant="ghost" size="xs" onClick={onCancel} />
    </div>
  );
}
