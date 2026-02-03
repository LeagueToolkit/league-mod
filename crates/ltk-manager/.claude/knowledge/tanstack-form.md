# TanStack Form Usage Guide

This project uses TanStack Form for type-safe form handling with pre-bound reusable components.

## Quick Start

```tsx
import { useAppForm } from "@/lib/form";
import { z } from "zod";

// Define validation schema (optional, but recommended)
const schema = z.object({
  name: z.string().min(1, "Required"),
  email: z.string().email("Invalid email"),
});

function MyForm() {
  const form = useAppForm({
    defaultValues: {
      name: "",
      email: "",
    },
    validators: {
      onChange: schema,
    },
    onSubmit: ({ value }) => {
      console.log(value);
    },
  });

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        form.handleSubmit();
      }}
    >
      <form.AppField name="name">
        {(field) => <field.TextField label="Name" required />}
      </form.AppField>

      <form.AppField name="email">
        {(field) => <field.TextField label="Email" type="email" />}
      </form.AppField>

      <form.Subscribe selector={(state) => ({ canSubmit: state.canSubmit })}>
        {({ canSubmit }) => (
          <Button type="submit" disabled={!canSubmit}>
            Submit
          </Button>
        )}
      </form.Subscribe>
    </form>
  );
}
```

## Architecture

The form system is located in `src/lib/form/`:

- `form-context.ts` - Creates form hook contexts for component binding
- `components.tsx` - Pre-bound field components (TextField, TextareaField, SubmitButton)
- `index.ts` - Main exports including `useAppForm` and `withForm`

## Available Components

### TextField

```tsx
<form.AppField name="fieldName">
  {(field) => (
    <field.TextField
      label="Label"
      description="Helper text"
      placeholder="Placeholder"
      required
      transform={(value) => value.toLowerCase()} // Optional value transformation
    />
  )}
</form.AppField>
```

### TextareaField

```tsx
<form.AppField name="description">
  {(field) => (
    <field.TextareaField label="Description" placeholder="Enter description..." rows={3} />
  )}
</form.AppField>
```

### SubmitButton (Form-aware)

```tsx
<form.AppForm>
  <form.SubmitButton>Submit</form.SubmitButton>
</form.AppForm>
```

Note: For more control, use the existing Button component with `form.Subscribe`:

```tsx
<form.Subscribe selector={(state) => ({ canSubmit: state.canSubmit, isValid: state.isValid })}>
  {({ canSubmit, isValid }) => <Button disabled={!canSubmit || !isValid}>Submit</Button>}
</form.Subscribe>
```

## Validation with Zod

```tsx
const schema = z.object({
  name: z
    .string()
    .min(1, "Required")
    .regex(/^[a-z]+$/, "Lowercase only"),
  age: z.number().min(0).max(150),
});

const form = useAppForm({
  defaultValues: { name: "", age: 0 },
  validators: { onChange: schema },
  onSubmit: ({ value }) => {
    /* ... */
  },
});
```

## Field Listeners

React to field changes:

```tsx
<form.AppField
  name="slug"
  listeners={{
    onChange: ({ value }) => {
      // Auto-generate display name when slug changes
      form.setFieldValue("displayName", generateDisplayName(value));
    },
  }}
>
  {(field) => <field.TextField label="Slug" />}
</form.AppField>
```

## Creating Custom Field Components

```tsx
import { useFieldContext } from "@/lib/form";

export function NumberField({ label }: { label: string }) {
  const field = useFieldContext<number>();

  return (
    <FieldRoot>
      <FieldLabel>{label}</FieldLabel>
      <FieldControl
        type="number"
        value={field.state.value}
        onChange={(e) => field.handleChange(Number(e.target.value))}
        onBlur={field.handleBlur}
      />
      {field.state.meta.errors.length > 0 && (
        <FieldError>{field.state.meta.errors.join(", ")}</FieldError>
      )}
    </FieldRoot>
  );
}
```

Then register in `src/lib/form/index.ts`:

```tsx
const { useAppForm, withForm } = createFormHook({
  fieldContext,
  formContext,
  fieldComponents: {
    TextField,
    TextareaField,
    NumberField, // Add here
  },
  formComponents: { SubmitButton },
});
```

## Breaking Large Forms into Pieces

Use `withForm` for reusable form sections:

```tsx
const AddressSection = withForm({
  defaultValues: {
    street: "",
    city: "",
    zip: "",
  },
  render: function Render({ form }) {
    return (
      <div>
        <form.AppField name="street">{(field) => <field.TextField label="Street" />}</form.AppField>
        <form.AppField name="city">{(field) => <field.TextField label="City" />}</form.AppField>
      </div>
    );
  },
});

// Usage in parent form
function ParentForm() {
  const form = useAppForm({
    defaultValues: { street: "", city: "", zip: "", name: "" },
    onSubmit: ({ value }) => {
      /* ... */
    },
  });

  return (
    <form>
      <form.AppField name="name">{(field) => <field.TextField label="Name" />}</form.AppField>
      <AddressSection form={form} />
    </form>
  );
}
```

## Form Reset

```tsx
function handleClose() {
  form.reset(); // Resets to defaultValues
  onClose();
}
```

## Key Differences from Manual State

| Manual State                                   | TanStack Form                      |
| ---------------------------------------------- | ---------------------------------- |
| `const [name, setName] = useState("")`         | Handled by `useAppForm`            |
| `const [error, setError] = useState<string>()` | Automatic via validators           |
| Manual validation in onChange                  | Declarative with Zod schemas       |
| Form submission checks                         | `form.handleSubmit()` + validators |

## References

- [TanStack Form Docs](https://tanstack.com/form/latest/docs/framework/react/quick-start)
- [Form Composition Guide](https://tanstack.com/form/latest/docs/framework/react/guides/form-composition)
