import type { AppError } from "./errors";

/**
 * A discriminated union representing either a successful result or an error.
 * This mirrors the Rust IpcResult type for type-safe IPC communication.
 */
export type Result<T, E = AppError> =
  | { ok: true; value: T }
  | { ok: false; error: E };

/**
 * Type guard to check if a Result is Ok (successful).
 */
export function isOk<T, E>(result: Result<T, E>): result is { ok: true; value: T } {
  return result.ok === true;
}

/**
 * Type guard to check if a Result is Err (error).
 */
export function isErr<T, E>(result: Result<T, E>): result is { ok: false; error: E } {
  return result.ok === false;
}

/**
 * Unwrap a Result, returning the value if Ok or throwing the error if Err.
 * Use sparingly - prefer pattern matching with isOk/isErr.
 */
export function unwrap<T, E>(result: Result<T, E>): T {
  if (isOk(result)) {
    return result.value;
  }
  throw result.error;
}

/**
 * Unwrap a Result, returning the value if Ok or a default value if Err.
 */
export function unwrapOr<T, E>(result: Result<T, E>, defaultValue: T): T {
  if (isOk(result)) {
    return result.value;
  }
  return defaultValue;
}

/**
 * Unwrap a Result, returning the value if Ok or calling a function with the error if Err.
 */
export function unwrapOrElse<T, E>(result: Result<T, E>, fn: (error: E) => T): T {
  if (isOk(result)) {
    return result.value;
  }
  return fn(result.error);
}

/**
 * Map a function over the Ok value of a Result.
 */
export function map<T, U, E>(result: Result<T, E>, fn: (value: T) => U): Result<U, E> {
  if (isOk(result)) {
    return { ok: true, value: fn(result.value) };
  }
  return result;
}

/**
 * Map a function over the Err value of a Result.
 */
export function mapErr<T, E, F>(result: Result<T, E>, fn: (error: E) => F): Result<T, F> {
  if (isErr(result)) {
    return { ok: false, error: fn(result.error) };
  }
  return result;
}

/**
 * Chain Result-returning operations together.
 */
export function andThen<T, U, E>(
  result: Result<T, E>,
  fn: (value: T) => Result<U, E>
): Result<U, E> {
  if (isOk(result)) {
    return fn(result.value);
  }
  return result;
}

/**
 * Match on a Result, calling one of two functions depending on the variant.
 */
export function match<T, E, R>(
  result: Result<T, E>,
  handlers: {
    ok: (value: T) => R;
    err: (error: E) => R;
  }
): R {
  if (isOk(result)) {
    return handlers.ok(result.value);
  }
  return handlers.err(result.error);
}

