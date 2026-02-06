import { Switch as BaseSwitch } from "@base-ui-components/react/switch";
import { forwardRef } from "react";
import { twMerge } from "tailwind-merge";

export type SwitchSize = "sm" | "md";

export interface SwitchProps extends Omit<BaseSwitch.Root.Props, "className"> {
  size?: SwitchSize;
  className?: string;
}

const trackClasses: Record<SwitchSize, string> = {
  sm: "h-5 w-9",
  md: "h-6 w-11",
};

const thumbClasses: Record<SwitchSize, string> = {
  sm: "top-0.5 left-0.5 h-4 w-4 data-[checked]:translate-x-4",
  md: "top-1 left-1 h-4 w-4 data-[checked]:translate-x-5",
};

export const Switch = forwardRef<HTMLSpanElement, SwitchProps>(
  ({ size = "md", className, ...props }, ref) => {
    return (
      <BaseSwitch.Root
        ref={ref}
        className={twMerge(
          "relative rounded-full transition-colors",
          "bg-surface-700 data-[checked]:bg-brand-500",
          trackClasses[size],
          className,
        )}
        {...props}
      >
        <BaseSwitch.Thumb
          className={twMerge(
            "absolute rounded-full bg-white transition-transform",
            thumbClasses[size],
          )}
        />
      </BaseSwitch.Root>
    );
  },
);
Switch.displayName = "Switch";
