import { Tabs as BaseTabs } from "@base-ui-components/react/tabs";
import { type ComponentPropsWithoutRef, forwardRef, type ReactNode } from "react";
import { twMerge } from "tailwind-merge";
import { match } from "ts-pattern";

export type TabsVariant = "default" | "pills";

// Root
export interface TabsRootProps extends Omit<BaseTabs.Root.Props, "className"> {
  className?: string;
}

export const TabsRoot = forwardRef<HTMLDivElement, TabsRootProps>(
  ({ className, ...props }, ref) => {
    return <BaseTabs.Root ref={ref} className={twMerge("flex flex-col", className)} {...props} />;
  },
);
TabsRoot.displayName = "Tabs.Root";

// List
export interface TabsListProps extends Omit<ComponentPropsWithoutRef<"div">, "className"> {
  variant?: TabsVariant;
  className?: string;
  children?: ReactNode;
}

export const TabsList = forwardRef<HTMLDivElement, TabsListProps>(
  ({ variant = "default", className, children, ...props }, ref) => {
    const variantClasses = match(variant)
      .with("default", () => "border-b border-surface-700 gap-0")
      .with("pills", () => "bg-surface-800 rounded-lg p-1 gap-1")
      .exhaustive();

    return (
      <BaseTabs.List
        ref={ref}
        className={twMerge("flex items-center", variantClasses, className)}
        {...props}
      >
        {children}
      </BaseTabs.List>
    );
  },
);
TabsList.displayName = "Tabs.List";

// Tab
export interface TabsTabProps extends Omit<BaseTabs.Tab.Props, "className"> {
  variant?: TabsVariant;
  className?: string;
  children?: ReactNode;
}

export const TabsTab = forwardRef<HTMLButtonElement, TabsTabProps>(
  ({ variant = "default", className, children, ...props }, ref) => {
    const baseClasses =
      "relative px-4 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 focus-visible:ring-offset-2 focus-visible:ring-offset-surface-900 disabled:pointer-events-none disabled:opacity-50";

    const variantClasses = match(variant)
      .with(
        "default",
        () =>
          "text-surface-400 hover:text-surface-200 data-[selected]:text-brand-400 data-[selected]:after:absolute data-[selected]:after:inset-x-0 data-[selected]:after:-bottom-px data-[selected]:after:h-0.5 data-[selected]:after:bg-brand-500",
      )
      .with(
        "pills",
        () =>
          "rounded-md text-surface-400 hover:text-surface-200 data-[selected]:bg-surface-700 data-[selected]:text-surface-100",
      )
      .exhaustive();

    return (
      <BaseTabs.Tab
        ref={ref}
        className={twMerge(baseClasses, variantClasses, className)}
        {...props}
      >
        {children}
      </BaseTabs.Tab>
    );
  },
);
TabsTab.displayName = "Tabs.Tab";

// Panel
export interface TabsPanelProps extends Omit<BaseTabs.Panel.Props, "className"> {
  className?: string;
  children?: ReactNode;
}

export const TabsPanel = forwardRef<HTMLDivElement, TabsPanelProps>(
  ({ className, children, ...props }, ref) => {
    return (
      <BaseTabs.Panel
        ref={ref}
        className={twMerge("mt-4 focus-visible:outline-none", className)}
        {...props}
      >
        {children}
      </BaseTabs.Panel>
    );
  },
);
TabsPanel.displayName = "Tabs.Panel";

// Indicator (optional animated indicator for default variant)
export interface TabsIndicatorProps extends Omit<BaseTabs.Indicator.Props, "className"> {
  className?: string;
}

export const TabsIndicator = forwardRef<HTMLSpanElement, TabsIndicatorProps>(
  ({ className, ...props }, ref) => {
    return (
      <BaseTabs.Indicator
        ref={ref}
        className={twMerge(
          "absolute bottom-0 h-0.5 bg-brand-500 transition-all duration-200",
          className,
        )}
        {...props}
      />
    );
  },
);
TabsIndicator.displayName = "Tabs.Indicator";

// Compound export
export const Tabs = {
  Root: TabsRoot,
  List: TabsList,
  Tab: TabsTab,
  Panel: TabsPanel,
  Indicator: TabsIndicator,
};
