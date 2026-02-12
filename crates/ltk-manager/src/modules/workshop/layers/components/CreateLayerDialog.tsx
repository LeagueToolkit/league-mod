import { z } from "zod";

import { Button, Dialog } from "@/components";
import { useAppForm } from "@/lib/form";

const createLayerSchema = z.object({
  name: z
    .string()
    .min(1, "Layer name is required")
    .regex(/^[a-z0-9-]+$/, "Name must be lowercase letters, numbers, and hyphens only")
    .refine(
      (val) => !val.startsWith("-") && !val.endsWith("-"),
      "Name cannot start or end with a hyphen",
    ),
  description: z.string(),
});

interface CreateLayerDialogProps {
  open: boolean;
  onClose: () => void;
  onSubmit: (name: string, description: string) => void;
  isPending?: boolean;
  existingNames: string[];
}

export function CreateLayerDialog({
  open,
  onClose,
  onSubmit,
  isPending,
  existingNames,
}: CreateLayerDialogProps) {
  const form = useAppForm({
    defaultValues: { name: "", description: "" },
    validators: {
      onChange: createLayerSchema,
    },
    onSubmit: ({ value }) => {
      onSubmit(value.name, value.description);
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
        <Dialog.Overlay size="sm">
          <Dialog.Header>
            <Dialog.Title>New Layer</Dialog.Title>
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
                validators={{
                  onChange: ({ value }) => {
                    if (existingNames.includes(value)) {
                      return "A layer with this name already exists";
                    }
                    return undefined;
                  },
                }}
              >
                {(field) => (
                  <field.TextField
                    label="Layer Name"
                    required
                    placeholder="high-res"
                    description="Lowercase letters, numbers, and hyphens only."
                    autoFocus
                    transform={(value) => value.toLowerCase()}
                  />
                )}
              </form.AppField>

              <form.AppField name="description">
                {(field) => (
                  <field.TextareaField
                    label="Description"
                    placeholder="Optional description for this layer..."
                    rows={2}
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
                    Create Layer
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
