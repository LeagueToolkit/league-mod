import { createFormHookContexts } from "@tanstack/react-form";

// Create form hook contexts for sharing form state between components
// useFieldContext is used in custom field components to access field state
// useFormContext is used in custom form components (like submit buttons) to access form state
export const { fieldContext, formContext, useFieldContext, useFormContext } =
  createFormHookContexts();
