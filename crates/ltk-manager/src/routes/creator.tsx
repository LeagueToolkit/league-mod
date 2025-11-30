import { createFileRoute } from "@tanstack/react-router";
import { Hammer } from "lucide-react";

export const Route = createFileRoute("/creator")({
  component: CreatorPage,
});

function CreatorPage() {
  return (
    <div className="flex h-full flex-col">
      <header className="border-surface-600 flex h-16 items-center border-b px-6">
        <h2 className="text-surface-100 text-xl font-semibold">Mod Creator</h2>
      </header>
      <div className="flex flex-1 items-center justify-center">
        <div className="text-center">
          <div className="mx-auto mb-4 flex h-20 w-20 items-center justify-center rounded-2xl">
            <Hammer className="text-surface-600 h-10 w-10" />
          </div>
          <h3 className="text-surface-300 mb-1 text-lg font-medium">Coming Soon</h3>
          <p className="text-surface-500">The mod creator is under development</p>
        </div>
      </div>
    </div>
  );
}
