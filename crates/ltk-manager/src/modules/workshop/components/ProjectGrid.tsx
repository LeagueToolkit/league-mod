import type { WorkshopProject } from "@/lib/tauri";

import { ProjectCard } from "./ProjectCard";
import type { ViewMode } from "./WorkshopToolbar";

interface ProjectGridProps {
  projects: WorkshopProject[];
  viewMode: ViewMode;
  onEdit: (project: WorkshopProject) => void;
  onPack: (project: WorkshopProject) => void;
  onDelete: (project: WorkshopProject) => void;
  onSetThumbnail: (project: WorkshopProject) => void;
  onStringOverrides: (project: WorkshopProject) => void;
}

export function ProjectGrid({
  projects,
  viewMode,
  onEdit,
  onPack,
  onDelete,
  onSetThumbnail,
  onStringOverrides,
}: ProjectGridProps) {
  return (
    <div
      className={
        viewMode === "grid"
          ? "grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
          : "space-y-2"
      }
    >
      {projects.map((project) => (
        <ProjectCard
          key={project.path}
          project={project}
          viewMode={viewMode}
          onEdit={onEdit}
          onPack={onPack}
          onDelete={onDelete}
          onSetThumbnail={onSetThumbnail}
          onStringOverrides={onStringOverrides}
        />
      ))}
    </div>
  );
}
