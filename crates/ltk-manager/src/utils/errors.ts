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
 * Context type for INVALID_PATH errors
 */
export interface InvalidPathContext {
  path: string;
}

/**
 * Context type for MOD_NOT_FOUND errors
 */
export interface ModNotFoundContext {
  modId: string;
}

/**
 * Get typed context from an INVALID_PATH error
 */
export function getInvalidPathContext(error: AppError): InvalidPathContext | undefined {
  if (error.code === "INVALID_PATH" && error.context) {
    return error.context as InvalidPathContext;
  }
  return undefined;
}

/**
 * Get typed context from a MOD_NOT_FOUND error
 */
export function getModNotFoundContext(error: AppError): ModNotFoundContext | undefined {
  if (error.code === "MOD_NOT_FOUND" && error.context) {
    return error.context as ModNotFoundContext;
  }
  return undefined;
}
