import type { ReactNode } from "react";
import { createContext, use } from "react";

import type { WorkshopProject } from "@/lib/tauri";

const ProjectContext = createContext<WorkshopProject | null>(null);

export function ProjectProvider({
  project,
  children,
}: {
  project: WorkshopProject;
  children: ReactNode;
}) {
  return <ProjectContext value={project}>{children}</ProjectContext>;
}

export function useProjectContext(): WorkshopProject {
  const project = use(ProjectContext);
  if (!project) {
    throw new Error("useProjectContext must be used within a ProjectProvider");
  }
  return project;
}
