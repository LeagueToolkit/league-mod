import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";
import { twMerge } from "tailwind-merge";

import {
  Field,
  FieldControl,
  FieldDescription,
  FieldError,
  FieldLabel,
  FieldRoot,
} from "@/components";

import { useFieldContext, useFormContext } from "./form-context";

// Re-export Field compound component for composition
export { Field };

// TextField - Pre-bound text input field component
export interface TextFieldProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "value" | "onChange" | "onBlur"
> {
  label?: string;
  description?: string;
  inputClassName?: string;
  /** Optional transform function applied to value before updating */
  transform?: (value: string) => string;
}

export function TextField({
  label,
  description,
  inputClassName,
  required,
  transform,
  ...props
}: TextFieldProps) {
  const field = useFieldContext<string>();
  const hasError = field.state.meta.errors.length > 0;

  return (
    <FieldRoot>
      {label && <FieldLabel required={required}>{label}</FieldLabel>}
      {description && <FieldDescription>{description}</FieldDescription>}
      <FieldControl
        value={field.state.value}
        onChange={(e) => {
          const value = transform ? transform(e.target.value) : e.target.value;
          field.handleChange(value);
        }}
        onBlur={field.handleBlur}
        hasError={hasError}
        className={inputClassName}
        {...props}
      />
      {hasError && <FieldError>{field.state.meta.errors.join(", ")}</FieldError>}
    </FieldRoot>
  );
}

// TextareaField - Pre-bound textarea field component
export interface TextareaFieldProps extends Omit<
  TextareaHTMLAttributes<HTMLTextAreaElement>,
  "value" | "onChange" | "onBlur"
> {
  label?: string;
  description?: string;
  textareaClassName?: string;
}

export function TextareaField({
  label,
  description,
  textareaClassName,
  required,
  className,
  ...props
}: TextareaFieldProps) {
  const field = useFieldContext<string>();
  const hasError = field.state.meta.errors.length > 0;

  return (
    <FieldRoot className={className}>
      {label && <FieldLabel required={required}>{label}</FieldLabel>}
      {description && <FieldDescription>{description}</FieldDescription>}
      <textarea
        value={field.state.value}
        onChange={(e) => field.handleChange(e.target.value)}
        onBlur={field.handleBlur}
        className={twMerge(
          "w-full rounded-lg border px-4 py-2.5 text-sm transition-colors",
          "bg-surface-700 text-surface-50 placeholder:text-surface-400",
          "border-surface-500 hover:border-surface-400",
          "focus:border-brand-500 focus:ring-1 focus:ring-brand-500 focus:outline-none",
          "disabled:cursor-not-allowed disabled:opacity-50",
          "min-h-[80px] resize-y",
          hasError && "border-red-500 focus:border-red-500 focus:ring-red-500",
          textareaClassName,
        )}
        {...props}
      />
      {hasError && <FieldError>{field.state.meta.errors.join(", ")}</FieldError>}
    </FieldRoot>
  );
}

// SubmitButton - Form-aware submit button
export interface SubmitButtonProps {
  children: React.ReactNode;
  className?: string;
  variant?: "filled" | "ghost" | "outline";
}

export function SubmitButton({ children, className, variant = "filled" }: SubmitButtonProps) {
  const form = useFormContext();

  return (
    <form.Subscribe
      selector={(state) => ({ isSubmitting: state.isSubmitting, canSubmit: state.canSubmit })}
    >
      {({ isSubmitting, canSubmit }) => {
        const baseStyles =
          "relative inline-flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-brand-500 focus:ring-offset-2 focus:ring-offset-surface-900 disabled:cursor-not-allowed disabled:opacity-50";

        const variantStyles = {
          filled: "bg-brand-600 text-white hover:bg-brand-700",
          ghost: "text-surface-300 hover:bg-surface-700 hover:text-surface-100",
          outline:
            "border border-surface-600 text-surface-300 hover:border-surface-500 hover:text-surface-100",
        };

        return (
          <button
            type="submit"
            disabled={!canSubmit || isSubmitting}
            className={twMerge(baseStyles, variantStyles[variant], className)}
          >
            {isSubmitting ? (
              <>
                <span className="absolute inset-0 flex items-center justify-center">
                  <span className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
                </span>
                <span className="invisible">{children}</span>
              </>
            ) : (
              children
            )}
          </button>
        );
      }}
    </form.Subscribe>
  );
}
