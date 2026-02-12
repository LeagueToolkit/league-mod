import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { LuCheck, LuImage, LuPencil, LuPlus, LuTrash2, LuX } from "react-icons/lu";

import { Button, IconButton, useToast } from "@/components";
import { useAppForm } from "@/lib/form";
import type { WorkshopAuthor } from "@/lib/tauri";
import {
  useProjectContext,
  useProjectThumbnail,
  useRenameProject,
  useSaveProjectConfig,
  useSetProjectThumbnail,
} from "@/modules/workshop";

export const Route = createFileRoute("/workshop/$projectName/")({
  component: ProjectOverview,
});

function ProjectOverview() {
  const project = useProjectContext();
  const navigate = useNavigate();
  const saveConfig = useSaveProjectConfig();
  const renameProject = useRenameProject();
  const { data: thumbnailUrl } = useProjectThumbnail(project.path, project.thumbnailPath);
  const setThumbnail = useSetProjectThumbnail();
  const toast = useToast();

  const [isEditingSlug, setIsEditingSlug] = useState(false);
  const [slugValue, setSlugValue] = useState(project.name);

  const [authors, setAuthors] = useState<WorkshopAuthor[]>(
    project.authors.length > 0 ? project.authors : [{ name: "", role: "" }],
  );

  const form = useAppForm({
    defaultValues: {
      displayName: project.displayName,
      version: project.version,
      description: project.description,
    },
    onSubmit: ({ value }) => {
      const filteredAuthors = authors.filter((a) => a.name.trim());
      saveConfig.mutate(
        {
          projectPath: project.path,
          displayName: value.displayName,
          version: value.version,
          description: value.description,
          authors: filteredAuthors,
        },
        {
          onSuccess: () => {
            toast.success("Project configuration saved");
          },
          onError: (err) => {
            toast.error(`Failed to save: ${err.message}`);
          },
        },
      );
    },
  });

  async function handleSetThumbnail() {
    const file = await open({
      multiple: false,
      filters: [
        {
          name: "Images",
          extensions: ["webp", "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "ico"],
        },
      ],
    });
    if (file) {
      setThumbnail.mutate(
        { projectPath: project.path, imagePath: file },
        { onError: (err) => toast.error(`Failed to set thumbnail: ${err.message}`) },
      );
    }
  }

  function handleAddAuthor() {
    setAuthors((prev) => [...prev, { name: "", role: "" }]);
  }

  function handleRemoveAuthor(index: number) {
    setAuthors((prev) => prev.filter((_, i) => i !== index));
  }

  function handleUpdateAuthor(index: number, field: "name" | "role", value: string) {
    setAuthors((prev) => {
      const updated = [...prev];
      updated[index] = { ...updated[index], [field]: value };
      return updated;
    });
  }

  function handleSaveSlug() {
    const trimmed = slugValue.trim();
    if (!trimmed || trimmed === project.name) {
      setIsEditingSlug(false);
      return;
    }
    renameProject.mutate(
      { projectPath: project.path, newName: trimmed },
      {
        onSuccess: (renamed) => {
          setIsEditingSlug(false);
          toast.success("Project renamed successfully");
          navigate({ to: "/workshop/$projectName", params: { projectName: renamed.name } });
        },
        onError: (err) => {
          toast.error(`Failed to rename: ${err.message}`);
        },
      },
    );
  }

  function handleCancelSlug() {
    setSlugValue(project.name);
    setIsEditingSlug(false);
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        form.handleSubmit();
      }}
      className="mx-auto max-w-3xl space-y-8"
    >
      {/* Thumbnail + Metadata side-by-side */}
      <div className="flex gap-8">
        {/* Thumbnail */}
        <div className="shrink-0 space-y-3">
          <div className="flex aspect-video w-72 items-center justify-center overflow-hidden rounded-lg border border-surface-600 bg-linear-to-br from-surface-700 to-surface-800">
            {thumbnailUrl ? (
              <img
                src={thumbnailUrl}
                alt="Project thumbnail"
                className="h-full w-full object-cover"
              />
            ) : (
              <LuImage className="h-10 w-10 text-surface-500" />
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            left={<LuImage className="h-4 w-4" />}
            onClick={handleSetThumbnail}
            loading={setThumbnail.isPending}
          >
            {project.thumbnailPath ? "Change" : "Set Thumbnail"}
          </Button>
        </div>

        {/* Metadata fields */}
        <div className="min-w-0 flex-1 space-y-4">
          <form.AppField name="displayName">
            {(field) => (
              <field.TextField label="Display Name" required placeholder="My Awesome Mod" />
            )}
          </form.AppField>

          <form.AppField name="version">
            {(field) => <field.TextField label="Version" required placeholder="1.0.0" />}
          </form.AppField>

          <form.AppField name="description">
            {(field) => (
              <field.TextareaField
                label="Description"
                placeholder="A brief description of your mod..."
                rows={3}
              />
            )}
          </form.AppField>
        </div>
      </div>

      {/* Authors */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="text-sm font-medium text-surface-200">Authors</h3>
            <p className="text-xs text-surface-400">People who contributed to this mod.</p>
          </div>
          <Button
            variant="outline"
            size="sm"
            left={<LuPlus className="h-4 w-4" />}
            onClick={handleAddAuthor}
          >
            Add Author
          </Button>
        </div>

        {authors.length > 0 && (
          <div className="space-y-2">
            <div className="flex items-center gap-2 px-1 text-xs font-medium text-surface-400">
              <div className="flex-1">Name</div>
              <div className="w-36">Role</div>
              <div className="w-9" />
            </div>

            {authors.map((author, index) => (
              <div key={index} className="flex items-center gap-2">
                <input
                  type="text"
                  value={author.name}
                  onChange={(e) => handleUpdateAuthor(index, "name", e.target.value)}
                  placeholder="Author name"
                  className="flex-1 rounded-lg border border-surface-500 bg-surface-700 px-4 py-2.5 text-sm text-surface-50 transition-colors placeholder:text-surface-400 hover:border-surface-400 focus:border-brand-500 focus:ring-1 focus:ring-brand-500 focus:outline-none"
                />
                <input
                  type="text"
                  value={author.role ?? ""}
                  onChange={(e) => handleUpdateAuthor(index, "role", e.target.value)}
                  placeholder="Role"
                  className="w-36 rounded-lg border border-surface-500 bg-surface-700 px-4 py-2.5 text-sm text-surface-50 transition-colors placeholder:text-surface-400 hover:border-surface-400 focus:border-brand-500 focus:ring-1 focus:ring-brand-500 focus:outline-none"
                />
                <IconButton
                  icon={<LuTrash2 className="h-4 w-4" />}
                  variant="ghost"
                  size="sm"
                  onClick={() => handleRemoveAuthor(index)}
                />
              </div>
            ))}
          </div>
        )}
      </div>

      <Button variant="filled" onClick={() => form.handleSubmit()} loading={saveConfig.isPending}>
        Save Changes
      </Button>

      {/* Read-only info */}
      <div className="rounded-lg border border-surface-700 bg-surface-800/50 p-4">
        <h3 className="mb-3 text-sm font-medium text-surface-300">Project Info</h3>
        <dl className="space-y-2 text-sm">
          <div className="flex items-center justify-between">
            <dt className="text-surface-400">Slug</dt>
            <dd className="flex items-center gap-2">
              {isEditingSlug ? (
                <>
                  <input
                    type="text"
                    value={slugValue}
                    onChange={(e) => setSlugValue(e.target.value.toLowerCase())}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") handleSaveSlug();
                      if (e.key === "Escape") handleCancelSlug();
                    }}
                    autoFocus
                    className="w-48 rounded border border-surface-500 bg-surface-700 px-2 py-1 font-mono text-sm text-surface-200 focus:border-brand-500 focus:ring-1 focus:ring-brand-500 focus:outline-none"
                  />
                  <IconButton
                    icon={<LuCheck className="h-3.5 w-3.5" />}
                    variant="ghost"
                    size="xs"
                    onClick={handleSaveSlug}
                    loading={renameProject.isPending}
                    aria-label="Save slug"
                  />
                  <IconButton
                    icon={<LuX className="h-3.5 w-3.5" />}
                    variant="ghost"
                    size="xs"
                    onClick={handleCancelSlug}
                    aria-label="Cancel editing"
                  />
                </>
              ) : (
                <>
                  <span className="font-mono text-surface-200">{project.name}</span>
                  <IconButton
                    icon={<LuPencil className="h-3 w-3" />}
                    variant="ghost"
                    size="xs"
                    onClick={() => {
                      setSlugValue(project.name);
                      setIsEditingSlug(true);
                    }}
                    aria-label="Edit slug"
                  />
                </>
              )}
            </dd>
          </div>
          <div className="flex justify-between">
            <dt className="text-surface-400">Path</dt>
            <dd className="max-w-sm truncate text-right font-mono text-surface-200">
              {project.path}
            </dd>
          </div>
          <div className="flex justify-between">
            <dt className="text-surface-400">Last Modified</dt>
            <dd className="text-surface-200">
              {new Date(project.lastModified).toLocaleDateString()}
            </dd>
          </div>
          <div className="flex justify-between">
            <dt className="text-surface-400">Layers</dt>
            <dd className="text-surface-200">{project.layers.length}</dd>
          </div>
        </dl>
      </div>
    </form>
  );
}
