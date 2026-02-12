import { useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { workshopKeys } from "../../api/keys";

interface ReorderLayersVariables {
  projectPath: string;
  layerNames: string[];
}

function reorderLayers(project: WorkshopProject, layerNames: string[]): WorkshopProject {
  const layerMap = new Map(project.layers.map((l) => [l.name, l]));
  // Base stays at priority 0, non-base layers get priority 1+
  const baseLayer = layerMap.get("base");
  const reorderedLayers: WorkshopProject["layers"] = [];
  if (baseLayer) {
    reorderedLayers.push({ ...baseLayer, priority: 0 });
  }
  for (let i = 0; i < layerNames.length; i++) {
    const layer = layerMap.get(layerNames[i]);
    if (layer) {
      reorderedLayers.push({ ...layer, priority: i + 1 });
    }
  }
  return { ...project, layers: reorderedLayers };
}

export function useReorderLayers() {
  const queryClient = useQueryClient();

  return useMutation<
    WorkshopProject,
    AppError,
    ReorderLayersVariables,
    { previousProjects?: WorkshopProject[] }
  >({
    mutationFn: async ({ projectPath, layerNames }) => {
      const result = await api.reorderProjectLayers(projectPath, layerNames);
      return unwrapForQuery(result);
    },
    onMutate: async ({ projectPath, layerNames }) => {
      await queryClient.cancelQueries({ queryKey: workshopKeys.projects() });

      const previousProjects = queryClient.getQueryData<WorkshopProject[]>(workshopKeys.projects());

      // Optimistic update on the projects list (source of truth for ProjectProvider)
      queryClient.setQueryData<WorkshopProject[]>(workshopKeys.projects(), (old) =>
        old?.map((p) => (p.path === projectPath ? reorderLayers(p, layerNames) : p)),
      );

      return { previousProjects };
    },
    onError: (_error, _variables, context) => {
      if (context?.previousProjects) {
        queryClient.setQueryData(workshopKeys.projects(), context.previousProjects);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: workshopKeys.projects() });
    },
  });
}
