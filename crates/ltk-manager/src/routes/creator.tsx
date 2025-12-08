import { createFileRoute } from "@tanstack/react-router";
import { LuHammer } from "react-icons/lu";

export const Route = createFileRoute("/creator")({
  component: CreatorPage,
});

function CreatorPage() {
  return (
    <div className="flex h-full flex-col">
      <header className="flex h-16 items-center border-b border-surface-600 px-6">
        <h2 className="text-xl font-semibold text-surface-100">Mod Creator</h2>
      </header>
      <div className="flex flex-1 items-center justify-center">
        <div className="text-center">
          <div className="mx-auto mb-4 flex h-20 w-20 items-center justify-center rounded-2xl">
            <LuHammer className="h-10 w-10 text-surface-600" />
          </div>
          <h3 className="mb-1 text-lg font-medium text-surface-300">Coming Soon</h3>
          <p className="text-surface-500">The mod creator is under development</p>
        </div>
      </div>
    </div>
  );
}
