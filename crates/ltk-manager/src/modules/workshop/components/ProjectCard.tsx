import { invoke } from "@tauri-apps/api/core";
import {
  LuEllipsisVertical,
  LuFolderOpen,
  LuImage,
  LuPackage,
  LuPencil,
  LuTrash2,
} from "react-icons/lu";

import { Button, IconButton, Menu } from "@/components";
import type { WorkshopProject } from "@/lib/tauri";

import { useProjectThumbnail } from "../api/useProjectThumbnail";

interface ProjectCardProps {
  project: WorkshopProject;
  viewMode: "grid" | "list";
  onEdit: (project: WorkshopProject) => void;
  onPack: (project: WorkshopProject) => void;
  onDelete: (project: WorkshopProject) => void;
  onSetThumbnail: (project: WorkshopProject) => void;
}

export function ProjectCard({
  project,
  viewMode,
  onEdit,
  onPack,
  onDelete,
  onSetThumbnail,
}: ProjectCardProps) {
  const { data: thumbnailUrl } = useProjectThumbnail(project.path, project.thumbnailPath);

  async function handleOpenLocation() {
    try {
      await invoke("reveal_in_explorer", { path: project.path });
    } catch (error) {
      console.error("Failed to open location:", error);
    }
  }

  function handleCardClick(e: React.MouseEvent) {
    // Don't trigger if clicking on menu
    if ((e.target as HTMLElement).closest("[data-no-click]")) {
      return;
    }
    onEdit(project);
  }

  if (viewMode === "list") {
    return (
      <div
        onClick={handleCardClick}
        className="group flex cursor-pointer items-center gap-4 rounded-lg border border-surface-700 bg-surface-900 p-4 transition-all hover:border-surface-600"
      >
        {/* Thumbnail */}
        <div className="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-linear-to-br from-surface-700 to-surface-800">
          {thumbnailUrl ? (
            <img src={thumbnailUrl} alt="" className="h-full w-full object-cover" />
          ) : (
            <span className="text-lg font-bold text-surface-500">
              {project.displayName.charAt(0).toUpperCase()}
            </span>
          )}
        </div>

        {/* Info */}
        <div className="min-w-0 flex-1">
          <h3 className="truncate font-medium text-surface-100">{project.displayName}</h3>
          <p className="truncate text-sm text-surface-500">
            v{project.version} • {project.authors.map((a) => a.name).join(", ") || "Unknown author"}
          </p>
        </div>

        {/* Actions */}
        <div className="flex items-center gap-2" data-no-click>
          <Button
            variant="outline"
            size="sm"
            left={<LuPackage className="h-4 w-4" />}
            onClick={() => onPack(project)}
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
                    icon={<LuPencil className="h-4 w-4" />}
                    onClick={() => onEdit(project)}
                  >
                    Edit Project
                  </Menu.Item>
                  <Menu.Item
                    icon={<LuPackage className="h-4 w-4" />}
                    onClick={() => onPack(project)}
                  >
                    Pack
                  </Menu.Item>
                  <Menu.Item
                    icon={<LuImage className="h-4 w-4" />}
                    onClick={() => onSetThumbnail(project)}
                  >
                    Set Thumbnail
                  </Menu.Item>
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
                    onClick={() => onDelete(project)}
                  >
                    Delete
                  </Menu.Item>
                </Menu.Popup>
              </Menu.Positioner>
            </Menu.Portal>
          </Menu.Root>
        </div>
      </div>
    );
  }

  // Grid view
  return (
    <div
      onClick={handleCardClick}
      className="group relative cursor-pointer rounded-xl border border-surface-600 bg-surface-800 transition-all hover:border-surface-400"
    >
      {/* Menu in top-right corner */}
      <div className="absolute top-2 right-2 z-10" data-no-click>
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
                <Menu.Item icon={<LuPencil className="h-4 w-4" />} onClick={() => onEdit(project)}>
                  Edit Project
                </Menu.Item>
                <Menu.Item icon={<LuPackage className="h-4 w-4" />} onClick={() => onPack(project)}>
                  Pack
                </Menu.Item>
                <Menu.Item
                  icon={<LuImage className="h-4 w-4" />}
                  onClick={() => onSetThumbnail(project)}
                >
                  Set Thumbnail
                </Menu.Item>
                <Menu.Item icon={<LuFolderOpen className="h-4 w-4" />} onClick={handleOpenLocation}>
                  Open Location
                </Menu.Item>
                <Menu.Separator />
                <Menu.Item
                  icon={<LuTrash2 className="h-4 w-4" />}
                  variant="danger"
                  onClick={() => onDelete(project)}
                >
                  Delete
                </Menu.Item>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>
      </div>

      {/* Thumbnail */}
      <div className="flex aspect-video items-center justify-center overflow-hidden rounded-t-xl bg-linear-to-br from-surface-700 to-surface-800">
        {thumbnailUrl ? (
          <img src={thumbnailUrl} alt="" className="h-full w-full object-cover" />
        ) : (
          <span className="text-4xl font-bold text-surface-400">
            {project.displayName.charAt(0).toUpperCase()}
          </span>
        )}
      </div>

      {/* Content */}
      <div className="p-3">
        <h3 className="mb-1 line-clamp-1 text-sm font-medium text-surface-100">
          {project.displayName}
        </h3>
        <div className="flex items-center text-xs text-surface-500">
          <span>v{project.version}</span>
          <span className="mx-1">•</span>
          <span className="flex-1 truncate">
            {project.authors.length > 0 ? project.authors[0].name : "Unknown"}
          </span>
        </div>
      </div>
    </div>
  );
}
