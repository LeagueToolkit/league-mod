import { Toast as BaseToast } from "@base-ui-components/react/toast";
import type { ReactNode } from "react";

import { ToastList } from "./Toast";

interface ToastProviderProps {
  children: ReactNode;
}

export function ToastProvider({ children }: ToastProviderProps) {
  return (
    <BaseToast.Provider timeout={5000}>
      {children}
      <BaseToast.Portal>
        <BaseToast.Viewport
          data-toast-viewport
          className="fixed right-4 bottom-4 z-[9999] flex w-full max-w-[420px] flex-col gap-2"
        >
          <ToastList />
        </BaseToast.Viewport>
      </BaseToast.Portal>
    </BaseToast.Provider>
  );
}
