import type { WorkshopLayer } from "@/lib/tauri";

export function getStringOverrideCount(layer: WorkshopLayer): number {
  return Object.values(layer.stringOverrides).reduce(
    (sum, localeOverrides) => sum + Object.keys(localeOverrides).length,
    0,
  );
}

export function LayerCard({ layer }: { layer: WorkshopLayer }) {
  const stringOverrideCount = getStringOverrideCount(layer);

  return (
    <div className="rounded-lg border border-surface-700 bg-surface-800/50 p-4">
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
}
