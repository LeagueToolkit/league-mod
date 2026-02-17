import { z } from "zod";

/**
 * Error codes that match the Rust ErrorCode enum.
 * Used for pattern matching on error types in the frontend.
 */
export type ErrorCode =
  | "IO"
  | "SERIALIZATION"
  | "MODPKG"
  | "LEAGUE_NOT_FOUND"
  | "INVALID_PATH"
  | "MOD_NOT_FOUND"
  | "VALIDATION_FAILED"
  | "INTERNAL_STATE"
  | "PATCHER_RUNNING"
  | "UNKNOWN";

/**
 * Structured error response from the backend.
 * Provides rich error information for proper error handling.
 */
export interface AppError {
  /** Machine-readable error code for pattern matching */
  code: ErrorCode;
  /** Human-readable error message */
  message: string;
  /** Optional contextual data (e.g., the invalid path, missing mod ID) */
  context?: unknown;
}

/**
 * Type guard to check if an error has a specific code.
 */
export function hasErrorCode<T extends ErrorCode>(
  error: AppError,
  code: T,
): error is AppError & { code: T } {
  return error.code === code;
}

/**
 * Schema for INVALID_PATH error context
 */
const InvalidPathContextSchema = z.object({
  path: z.string(),
});

/**
 * Context type for INVALID_PATH errors
 */
export type InvalidPathContext = z.infer<typeof InvalidPathContextSchema>;

/**
 * Schema for MOD_NOT_FOUND error context
 */
const ModNotFoundContextSchema = z.object({
  modId: z.string(),
});

/**
 * Context type for MOD_NOT_FOUND errors
 */
export type ModNotFoundContext = z.infer<typeof ModNotFoundContextSchema>;

/**
 * Get typed and validated context from an INVALID_PATH error.
 * Returns undefined if the error code doesn't match or context validation fails.
 */
export function getInvalidPathContext(error: AppError): InvalidPathContext | undefined {
  if (error.code !== "INVALID_PATH" || !error.context) {
    return undefined;
  }
  const result = InvalidPathContextSchema.safeParse(error.context);
  return result.success ? result.data : undefined;
}

/**
 * Get typed and validated context from a MOD_NOT_FOUND error.
 * Returns undefined if the error code doesn't match or context validation fails.
 */
export function getModNotFoundContext(error: AppError): ModNotFoundContext | undefined {
  if (error.code !== "MOD_NOT_FOUND" || !error.context) {
    return undefined;
  }
  const result = ModNotFoundContextSchema.safeParse(error.context);
  return result.success ? result.data : undefined;
}
