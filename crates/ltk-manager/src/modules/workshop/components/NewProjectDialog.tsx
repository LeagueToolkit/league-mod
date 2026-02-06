import { Dialog } from "@base-ui-components/react";
import { LuX } from "react-icons/lu";
import { z } from "zod";

import { Button, IconButton } from "@/components";
import { useAppForm } from "@/lib/form";
import type { CreateProjectArgs } from "@/lib/tauri";

// Validation schema for the project form
const projectSchema = z.object({
  name: z
    .string()
    .min(1, "Project name is required")
    .regex(/^[a-z0-9-]+$/, "Name must be lowercase letters, numbers, and hyphens only")
    .refine(
      (val) => !val.startsWith("-") && !val.endsWith("-"),
      "Name cannot start or end with a hyphen",
    ),
  displayName: z.string(),
  description: z.string(),
  authorName: z.string(),
});

interface NewProjectDialogProps {
  open: boolean;
  onClose: () => void;
  onSubmit: (args: CreateProjectArgs) => void;
  isPending?: boolean;
}

function generateDisplayName(slug: string) {
  return slug
    .split("-")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

export function NewProjectDialog({ open, onClose, onSubmit, isPending }: NewProjectDialogProps) {
  const form = useAppForm({
    defaultValues: {
      name: "",
      displayName: "",
      description: "",
      authorName: "",
    },
    validators: {
      onChange: projectSchema,
    },
    onSubmit: ({ value }) => {
      onSubmit({
        name: value.name,
        displayName: value.displayName || generateDisplayName(value.name),
        description: value.description,
        authors: value.authorName ? [value.authorName] : [],
      });
    },
  });

  function handleClose() {
    form.reset();
    onClose();
  }

  return (
    <Dialog.Root open={open} onOpenChange={(open) => !open && handleClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm" />
        <Dialog.Popup className="fixed top-1/2 left-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-xl border border-surface-600 bg-surface-800 shadow-2xl">
          <div className="flex items-center justify-between border-b border-surface-600 px-6 py-4">
            <Dialog.Title className="text-lg font-semibold text-surface-100">
              New Project
            </Dialog.Title>
            <IconButton
              icon={<LuX className="h-5 w-5" />}
              variant="ghost"
              size="sm"
              onClick={handleClose}
            />
          </div>

          <form
            onSubmit={(e) => {
              e.preventDefault();
              form.handleSubmit();
            }}
          >
            <div className="space-y-4 px-6 py-4">
              {/* Project Name (slug) */}
              <form.AppField
                name="name"
                listeners={{
                  onChange: ({ value }) => {
                    // Auto-generate display name when name changes
                    const currentDisplayName = form.getFieldValue("displayName");
                    const previousName = form.state.values.name;
                    if (
                      !currentDisplayName ||
                      currentDisplayName === generateDisplayName(previousName)
                    ) {
                      form.setFieldValue("displayName", generateDisplayName(value));
                    }
                  },
                }}
              >
                {(field) => (
                  <field.TextField
                    label="Project Name"
                    required
                    placeholder="my-awesome-mod"
                    description="Lowercase letters, numbers, and hyphens only. This will be the folder name."
                    autoFocus
                    transform={(value) => value.toLowerCase()}
                  />
                )}
              </form.AppField>

              {/* Display Name */}
              <form.AppField name="displayName">
                {(field) => <field.TextField label="Display Name" placeholder="My Awesome Mod" />}
              </form.AppField>

              {/* Author */}
              <form.AppField name="authorName">
                {(field) => <field.TextField label="Author" placeholder="Your name" />}
              </form.AppField>

              {/* Description */}
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

            <div className="flex justify-end gap-3 border-t border-surface-600 px-6 py-4">
              <Button variant="ghost" onClick={handleClose}>
                Cancel
              </Button>
              <form.Subscribe
                selector={(state) => ({ canSubmit: state.canSubmit, isValid: state.isValid })}
              >
                {({ canSubmit, isValid }) => (
                  <Button
                    variant="filled"
                    loading={isPending}
                    disabled={!canSubmit || !isValid}
                    onClick={() => form.handleSubmit()}
                  >
                    Create Project
                  </Button>
                )}
              </form.Subscribe>
            </div>
          </form>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
