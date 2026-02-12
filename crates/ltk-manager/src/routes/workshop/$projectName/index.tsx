import { createFileRoute } from "@tanstack/react-router";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { LuImage, LuPlus, LuTrash2 } from "react-icons/lu";

import { Button, IconButton, useToast } from "@/components";
import { useAppForm } from "@/lib/form";
import type { WorkshopAuthor } from "@/lib/tauri";
import {
  useProjectContext,
  useProjectThumbnail,
  useSaveProjectConfig,
  useSetProjectThumbnail,
} from "@/modules/workshop";

export const Route = createFileRoute("/workshop/$projectName/")({
  component: ProjectOverview,
});

function ProjectOverview() {
  const project = useProjectContext();
  const saveConfig = useSaveProjectConfig();
  const { data: thumbnailUrl } = useProjectThumbnail(project.path, project.thumbnailPath);
  const setThumbnail = useSetProjectThumbnail();
  const toast = useToast();

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

  return (
    <div className="mx-auto max-w-2xl space-y-8">
      {/* Metadata Form */}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          form.handleSubmit();
        }}
        className="space-y-6"
      >
        <div>
          <h2 className="text-base font-semibold text-surface-100">Metadata</h2>
          <p className="mt-1 text-sm text-surface-400">Configure your project details.</p>
        </div>

        <div className="space-y-4">
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
      </form>

      {/* Thumbnail */}
      <div className="space-y-3">
        <div>
          <h2 className="text-base font-semibold text-surface-100">Thumbnail</h2>
          <p className="mt-1 text-sm text-surface-400">
            Set a thumbnail image for your mod. Recommended aspect ratio is 16:9.
          </p>
        </div>
        <div className="flex items-end gap-4">
          <div className="flex h-28 w-48 items-center justify-center overflow-hidden rounded-lg border border-surface-600 bg-linear-to-br from-surface-700 to-surface-800">
            {thumbnailUrl ? (
              <img
                src={thumbnailUrl}
                alt="Project thumbnail"
                className="h-full w-full object-cover"
              />
            ) : (
              <LuImage className="h-8 w-8 text-surface-500" />
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
      </div>

      {/* Read-only info */}
      <div className="rounded-lg border border-surface-700 bg-surface-800/50 p-4">
        <h3 className="mb-3 text-sm font-medium text-surface-300">Project Info</h3>
        <dl className="space-y-2 text-sm">
          <div className="flex justify-between">
            <dt className="text-surface-400">Slug</dt>
            <dd className="font-mono text-surface-200">{project.name}</dd>
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
    </div>
  );
}
