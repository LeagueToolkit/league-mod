import { createFileRoute } from "@tanstack/react-router";
import { LuLayers } from "react-icons/lu";

import { useProjectContext } from "@/modules/workshop";

export const Route = createFileRoute("/workshop/$projectName/layers")({
  component: ProjectLayers,
});

function ProjectLayers() {
  const project = useProjectContext();
  const layers = [...project.layers].sort((a, b) => a.priority - b.priority);

  if (layers.length === 0) {
    return (
      <div className="flex h-48 flex-col items-center justify-center text-center">
        <LuLayers className="mb-3 h-10 w-10 text-surface-600" />
        <h3 className="text-sm font-medium text-surface-300">No layers</h3>
        <p className="mt-1 text-sm text-surface-500">This project has no layers configured.</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-2xl space-y-6">
      <div>
        <h2 className="text-base font-semibold text-surface-100">Layers</h2>
        <p className="mt-1 text-sm text-surface-400">
          Layers are applied in priority order. Higher priority layers override lower ones.
        </p>
      </div>

      <div className="space-y-3">
        {layers.map((layer) => {
          const stringOverrideCount = Object.values(layer.stringOverrides).reduce(
            (sum, localeOverrides) => sum + Object.keys(localeOverrides).length,
            0,
          );

          return (
            <div
              key={layer.name}
              className="rounded-lg border border-surface-700 bg-surface-800/50 p-4"
            >
              <div className="flex items-center gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <h3 className="font-medium text-surface-100">{layer.name}</h3>
                    <span className="rounded-full bg-surface-700 px-2 py-0.5 text-xs text-surface-400">
                      Priority {layer.priority}
                    </span>
                  </div>
                  {layer.description && (
                    <p className="mt-1 text-sm text-surface-400">{layer.description}</p>
                  )}
                </div>

                {stringOverrideCount > 0 && (
                  <span className="shrink-0 text-xs text-surface-400">
                    {stringOverrideCount} string override{stringOverrideCount !== 1 ? "s" : ""}
                  </span>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
