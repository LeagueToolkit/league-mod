import { isErr, type Result } from "./result";

/**
 * Unwrap a Result for use with TanStack Query.
 * Throws the error if Result is Err, allowing Query to catch it.
 *
 * @example
 * ```ts
 * const result = await api.getInstalledMods();
 * return unwrapForQuery(result); // throws if error
 * ```
 */
export function unwrapForQuery<T, E>(result: Result<T, E>): T {
  if (isErr(result)) {
    throw result.error;
  }
  return result.value;
}

/**
 * Wrap an API call for use with TanStack Query's queryFn.
 * Returns a function that throws on error, which Query expects.
 *
 * @example
 * ```ts
 * useQuery({
 *   queryKey: ["mods"],
 *   queryFn: queryFn(api.getInstalledMods),
 * });
 * ```
 */
export function queryFn<T, E>(fn: () => Promise<Result<T, E>>): () => Promise<T> {
  return async () => {
    const result = await fn();
    return unwrapForQuery(result);
  };
}

/**
 * Wrap an API call with arguments for use with TanStack Query's queryFn.
 *
 * @example
 * ```ts
 * useQuery({
 *   queryKey: ["mod", modId],
 *   queryFn: queryFnWithArgs(api.getMod, modId),
 * });
 * ```
 */
export function queryFnWithArgs<T, E, Args extends unknown[]>(
  fn: (...args: Args) => Promise<Result<T, E>>,
  ...args: Args
): () => Promise<T> {
  return async () => {
    const result = await fn(...args);
    return unwrapForQuery(result);
  };
}

/**
 * Create a mutation function that unwraps the Result.
 *
 * @example
 * ```ts
 * useMutation({
 *   mutationFn: mutationFn(api.installMod),
 * });
 * ```
 */
export function mutationFn<T, E, TVariables>(
  fn: (variables: TVariables) => Promise<Result<T, E>>,
): (variables: TVariables) => Promise<T> {
  return async (variables) => {
    const result = await fn(variables);
    return unwrapForQuery(result);
  };
}
