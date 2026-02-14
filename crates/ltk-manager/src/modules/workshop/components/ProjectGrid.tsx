import type { WorkshopProject } from "@/lib/tauri";

import { ProjectCard } from "./ProjectCard";
import type { ViewMode } from "./WorkshopToolbar";

interface ProjectGridProps {
  projects: WorkshopProject[];
  viewMode: ViewMode;
  onEdit: (project: WorkshopProject) => void;
  onPack: (project: WorkshopProject) => void;
  onDelete: (project: WorkshopProject) => void;
}

export function ProjectGrid({ projects, viewMode, onEdit, onPack, onDelete }: ProjectGridProps) {
  return (
    <div
      className={
        viewMode === "grid"
          ? "grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] gap-4"
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
        />
      ))}
    </div>
  );
}
