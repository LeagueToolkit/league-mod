import { createFormHook, formOptions } from "@tanstack/react-form";

import { SelectField, SubmitButton, TextareaField, TextField } from "./components";
import { fieldContext, formContext, useFieldContext, useFormContext } from "./form-context";

// Create the app-wide form hook with pre-bound components
// This provides type-safe form handling with reusable field components
const { useAppForm, withForm } = createFormHook({
  fieldContext,
  formContext,
  fieldComponents: {
    TextField,
    TextareaField,
    SelectField,
  },
  formComponents: {
    SubmitButton,
  },
});

// Re-export everything for convenient imports
export {
  formOptions,
  // Pre-built field components
  SelectField,
  SubmitButton,
  TextareaField,
  TextField,
  // Form hook and utilities
  useAppForm,
  // Context hooks for custom components
  useFieldContext,
  useFormContext,
  withForm,
};
