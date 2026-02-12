import { createFileRoute, Link, Outlet, useNavigate } from "@tanstack/react-router";
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { LuArrowLeft, LuEllipsisVertical, LuFolderOpen, LuPackage, LuTrash2 } from "react-icons/lu";

import { Button, IconButton, Menu, NavTabs } from "@/components";
import type { PackResult } from "@/lib/tauri";
import {
  DeleteConfirmDialog,
  LoadingState,
  PackDialog,
  ProjectProvider,
  useDeleteProject,
  usePackProject,
  useValidateProject,
  useWorkshopProjects,
} from "@/modules/workshop";

export const Route = createFileRoute("/workshop/$projectName")({
  component: ProjectDetailLayout,
});

function ProjectDetailLayout() {
  const { projectName } = Route.useParams();
  const navigate = useNavigate();

  const { data: projects, isLoading } = useWorkshopProjects();
  const project = projects?.find((p) => p.name === projectName);

  // Dialog state
  const [packDialogOpen, setPackDialogOpen] = useState(false);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [packResult, setPackResult] = useState<PackResult | null>(null);

  const deleteProject = useDeleteProject();
  const packProject = usePackProject();
  const { data: validation, isLoading: validationLoading } = useValidateProject(
    project?.path ?? "",
    packDialogOpen,
  );

  if (isLoading) {
    return <LoadingState />;
  }

  if (!project) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4">
        <p className="text-surface-400">Project not found: {projectName}</p>
        <Link to="/workshop">
          <Button variant="outline" left={<LuArrowLeft className="h-4 w-4" />}>
            Back to Workshop
          </Button>
        </Link>
      </div>
    );
  }

  function handlePack(format: "modpkg" | "fantome") {
    if (!project) return;
    packProject.mutate(
      { projectPath: project.path, format },
      {
        onSuccess: setPackResult,
        onError: (err) => console.error("Failed to pack project:", err.message),
      },
    );
  }

  function handleClosePackDialog() {
    setPackDialogOpen(false);
    setPackResult(null);
  }

  function handleDeleteProject() {
    if (!project) return;
    deleteProject.mutate(project.path, {
      onSuccess: () => {
        navigate({ to: "/workshop" });
      },
      onError: (err) => console.error("Failed to delete project:", err.message),
    });
  }

  async function handleOpenLocation() {
    if (!project) return;
    try {
      await invoke("reveal_in_explorer", { path: project.path });
    } catch (error) {
      console.error("Failed to open location:", error);
    }
  }

  const tabs = [
    { to: "/workshop/$projectName", params: { projectName }, label: "Overview", exact: true },
    { to: "/workshop/$projectName/strings", params: { projectName }, label: "Strings" },
    { to: "/workshop/$projectName/layers", params: { projectName }, label: "Layers" },
  ];

  return (
    <ProjectProvider project={project}>
      <div className="flex h-full flex-col">
        {/* Header */}
        <div className="flex items-center gap-3 border-b border-surface-700 px-6 py-3">
          <Link to="/workshop">
            <IconButton
              icon={<LuArrowLeft className="h-4 w-4" />}
              variant="ghost"
              size="sm"
              aria-label="Back to Workshop"
            />
          </Link>

          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <h1 className="truncate text-lg font-semibold text-surface-100">
                {project.displayName}
              </h1>
              <span className="shrink-0 rounded-full bg-surface-700 px-2 py-0.5 text-xs text-surface-400">
                v{project.version}
              </span>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              left={<LuPackage className="h-4 w-4" />}
              onClick={() => {
                setPackResult(null);
                setPackDialogOpen(true);
              }}
            >
              Pack
            </Button>
            <Menu.Root>
              <Menu.Trigger
                render={
                  <IconButton
                    icon={<LuEllipsisVertical className="h-4 w-4" />}
                    variant="ghost"
                    size="sm"
                  />
                }
              />
              <Menu.Portal>
                <Menu.Positioner>
                  <Menu.Popup>
                    <Menu.Item
                      icon={<LuFolderOpen className="h-4 w-4" />}
                      onClick={handleOpenLocation}
                    >
                      Open Location
                    </Menu.Item>
                    <Menu.Separator />
                    <Menu.Item
                      icon={<LuTrash2 className="h-4 w-4" />}
                      variant="danger"
                      onClick={() => setDeleteDialogOpen(true)}
                    >
                      Delete
                    </Menu.Item>
                  </Menu.Popup>
                </Menu.Positioner>
              </Menu.Portal>
            </Menu.Root>
          </div>
        </div>

        {/* NavTabs */}
        <NavTabs tabs={tabs} />

        {/* Content */}
        <div className="flex-1 overflow-auto p-6">
          <Outlet />
        </div>
      </div>

      {/* Dialogs */}
      <PackDialog
        open={packDialogOpen}
        project={project}
        validation={validation ?? null}
        validationLoading={validationLoading}
        onClose={handleClosePackDialog}
        onPack={handlePack}
        isPacking={packProject.isPending}
        packResult={packResult}
      />

      <DeleteConfirmDialog
        open={deleteDialogOpen}
        project={project}
        onClose={() => setDeleteDialogOpen(false)}
        onConfirm={handleDeleteProject}
        isPending={deleteProject.isPending}
      />
    </ProjectProvider>
  );
}
