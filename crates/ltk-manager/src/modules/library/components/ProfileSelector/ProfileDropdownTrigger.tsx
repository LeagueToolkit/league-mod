import { LuChevronDown } from "react-icons/lu";

import { Button } from "@/components";

interface ProfileDropdownTriggerProps {
  activeProfileName: string;
  isOpen: boolean;
  onClick: () => void;
}

export function ProfileDropdownTrigger({
  activeProfileName,
  isOpen,
  onClick,
}: ProfileDropdownTriggerProps) {
  return (
    <Button
      variant="default"
      size="sm"
      onClick={onClick}
      right={
        <LuChevronDown
          className={`h-4 w-4 text-surface-400 transition-transform ${isOpen ? "rotate-180" : ""}`}
        />
      }
    >
      {activeProfileName}
    </Button>
  );
}
