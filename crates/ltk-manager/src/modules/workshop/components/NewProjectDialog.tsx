import { z } from "zod";

import { Button, Dialog } from "@/components";
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
        <Dialog.Backdrop />
        <Dialog.Overlay>
          <Dialog.Header>
            <Dialog.Title>New Project</Dialog.Title>
            <Dialog.Close />
          </Dialog.Header>

          <form
            onSubmit={(e) => {
              e.preventDefault();
              form.handleSubmit();
            }}
          >
            <Dialog.Body className="space-y-4">
              <form.AppField
                name="name"
                listeners={{
                  onChange: ({ value }) => {
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

              <form.AppField name="displayName">
                {(field) => <field.TextField label="Display Name" placeholder="My Awesome Mod" />}
              </form.AppField>

              <form.AppField name="authorName">
                {(field) => <field.TextField label="Author" placeholder="Your name" />}
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
            </Dialog.Body>

            <Dialog.Footer>
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
            </Dialog.Footer>
          </form>
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
