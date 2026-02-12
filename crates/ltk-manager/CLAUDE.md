# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

See also the workspace-level [../../CLAUDE.md](../../CLAUDE.md) for full architecture details, patterns, and constraints. This file covers ltk-manager-specific context.

## Commands

All commands run from `crates/ltk-manager/`.

```bash
# Full dev mode (Rust backend + React frontend with hot reload)
pnpm tauri dev

# Frontend only (skip Rust rebuild, faster iteration on UI)
pnpm dev

# Type check / lint / format / all three
pnpm typecheck
pnpm lint
pnpm format
pnpm check          # typecheck + lint + format:check

# Production build
pnpm tauri build

# Rust-only operations (from workspace root)
cargo clippy -p ltk-manager
cargo fmt -p ltk-manager

# Verbose backend logging
RUST_LOG=ltk_manager=trace,tauri=info pnpm tauri dev
```

## Editing Rules

**Always read files before editing them.** Never assume file contents from memory or prior context. When making bulk edits across multiple files, read all target files first, then perform edits.

## Code Style

From `.cursorrules`: avoid trivially descriptive comments. Only comment non-obvious business logic, workarounds, edge cases, or "why" decisions. Document all public Rust APIs with `///` doc comments.

### JSX Conditional Rendering

**Avoid ternary operators in JSX.** Use early returns or `{condition && <Component />}` instead.

```tsx
// Good — early return
if (isLoading) return <LoadingState />;
if (error) return <ErrorState error={error} />;
return <Content />;

// Good — single-line conditional
{
  hasItems && <ItemList items={items} />;
}

// Bad — ternary in JSX
{
  isLoading ? <LoadingState /> : error ? <ErrorState /> : <Content />;
}
```

### Import Conventions

**Always import from barrel exports, never from subdirectories.** This keeps import paths stable and encapsulates internal structure.

- **Global components:** import from `@/components`, not `@/components/Button`, `@/components/Toast`, etc.
- **Modules:** import from `@/modules/{module}`, not `@/modules/{module}/components` or `@/modules/{module}/api`.

```ts
// Good
import { Button, IconButton, useToast } from "@/components";
import { ModCard, useInstalledMods } from "@/modules/library";

// Bad — reaches into internals
import { Button } from "@/components/Button";
import { useToast } from "@/components/Toast";
import { ModCard } from "@/modules/library/components";
```

## Backend (Rust) — `src-tauri/src/`

### Module Layout

- `main.rs` — Tauri setup, command registration in `generate_handler![]`, logging init
- `error.rs` — `AppError`, `AppErrorResponse`, `IpcResult<T>`, `MutexResultExt`
- `state.rs` — `SettingsState(Mutex<Settings>)`, settings persistence
- `commands/` — `#[tauri::command]` wrappers (one file per domain: `mods.rs`, `profiles.rs`, `patcher.rs`, `settings.rs`, `workshop.rs`, `shell.rs`, `app.rs`)
- `mods/mod.rs` — Business logic for mod install/uninstall/toggle, profile CRUD, library index management
- `overlay/` — Overlay building, content providers (`modpkg_content.rs`, `fantome_content.rs`)
- `patcher/` — Patcher lifecycle (start/stop/status), thread management with `Arc<AtomicBool>` stop flag
- `legacy_patcher/` — FFI integration with `cslol-dll.dll`

### State

Two Tauri-managed states:

- `SettingsState` — App settings (league path, storage path, theme). Access via `State<SettingsState>`, lock with `.0.lock().mutex_err()?.clone()`.
- `PatcherState` — Patcher thread handle and stop flag. Access via `State<PatcherState>`.

### Error Codes

`ErrorCode` enum variants (serialized as `SCREAMING_SNAKE_CASE`): `Io`, `Serialization`, `Modpkg`, `LeagueNotFound`, `InvalidPath`, `ModNotFound`, `ValidationFailed`, `InternalState`, `MutexLockFailed`, `Unknown`, `WorkshopNotConfigured`, `ProjectNotFound`, `ProjectAlreadyExists`, `PackFailed`, `Wad`.

Errors can carry JSON context: `AppErrorResponse::new(code, msg).with_context(json!({ "modId": id }))`.

## Frontend (React + TypeScript) — `src/`

### Key Files

- `lib/tauri.ts` — All Tauri command bindings (`api` object), TypeScript types matching Rust structs, `invokeResult<T>()` wrapper
- `utils/result.ts` — `Result<T, E>` discriminated union, `isOk`, `isErr`, `unwrap`, `match`
- `utils/query.ts` — `queryFn()`, `queryFnWithArgs()`, `mutationFn()`, `unwrapForQuery()` bridges between `Result<T>` and TanStack Query
- `utils/errors.ts` — `AppError` interface, `ErrorCode` type, `hasErrorCode()` guard, context extractors with Zod
- `stores/` — Zustand stores for client-only state

### Adding a New Tauri Command (Checklist)

1. Business logic in `src-tauri/src/{module}/` → returns `AppResult<T>`
2. Command wrapper in `src-tauri/src/commands/{module}.rs` → returns `IpcResult<T>` via `.into()`
3. Export in `src-tauri/src/commands/mod.rs`
4. Register in `main.rs` `generate_handler![]`
5. Add TS types + `api.myCommand` in `src/lib/tauri.ts`
6. Create hook in `src/modules/{module}/api/useMyCommand.ts`
7. Export through `src/modules/{module}/api/index.ts` → `src/modules/{module}/index.ts`

### Tauri Event Listening

For backend-to-frontend events (e.g., overlay progress), use `listen<T>()` from `@tauri-apps/api/event` in a `useEffect` with cleanup via `unlisten()`. See `modules/patcher/api/useOverlayProgress.ts` for the pattern.

### Routing

TanStack Router with file-based routing in `src/routes/`. Route tree is auto-generated in `routeTree.gen.ts`. The root route (`__root.tsx`) checks setup status and redirects to `/settings` on first run.

### Component Library (`src/components/`)

**ALWAYS use reusable components from `@/components` instead of native HTML or raw base-ui imports.** Module code should never import from `@base-ui-components/react` directly — all base-ui primitives must be wrapped in `src/components/` first.

**Available components:**
| Component | Usage | Base-UI Primitive |
| ------------------------------------------------------------------- | -------------------------------- | --------------------- |
| `Button`, `IconButton` | All clickable actions | `Button` |
| `Field`, `FormField`, `TextareaField` | All form inputs (text, textarea) | `Field` |
| `Checkbox`, `CheckboxGroup` | Boolean/multi-select inputs | `Checkbox` |
| `RadioGroup` (compound: `Root`, `Label`, `Options`, `Card`, `Item`) | Mutually exclusive choices | `Radio`, `RadioGroup` |
| `Tabs` (compound: `Root`, `List`, `Tab`, `Panel`, `Indicator`) | Tabbed content | `Tabs` |
| `Tooltip`, `SimpleTooltip` | Hover information | `Tooltip` |
| `Toast`, `ToastProvider`, `useToast()` | Notifications | `Toast` |
| `Switch` (sizes: `sm`, `md`) | Toggle on/off | `Switch` |
| `Menu` (compound: `Root`, `Trigger`, `Portal`, `Positioner`, `Popup`, `Item`, `Separator`, `Group`, `GroupLabel`) | Dropdown/context menus | `Menu` |
| `Select`, `SelectField` (compound + simplified), TanStack Form: `field.SelectField` | Dropdown select inputs | `Select` |
| `Popover` (compound: `Root`, `Trigger`, `Portal`, `Backdrop`, `Positioner`, `Popup`, `Arrow`, `Title`, `Description`, `Close`) | Positioned popover panels | `Popover` |

**Not yet wrapped (needed):**
| Component | Priority | Current Workaround |
| ---------------------- | -------- | -------------------------------------------------- |
| `Progress` | LOW | Custom overlay progress rendering |
| `ScrollArea` | LOW | Native scrollbars |

When adding a new base-ui component:

1. Create wrapper in `src/components/NewComponent.tsx`
2. Export from `src/components/index.ts`
3. Import in modules via `@/components`, never from `@base-ui-components/react` directly

### Key Dependencies

- `@base-ui-components/react` — Headless UI primitives (wrapped via `src/components/`)
- `@tanstack/react-form` + `zod` — Form management with validation
- `ts-pattern` — Exhaustive pattern matching
- `zustand` — Client-side state (not for server state — use TanStack Query)
- `react-icons` — Icon library
- `tailwind-merge` — Merging Tailwind classes in component variants

### Styling

Tailwind CSS v4 via `@tailwindcss/vite` plugin. Theme uses CSS custom properties with HSL-based accent color system (`--accent-hue`). Color tokens: `surface-{50..950}` for neutrals, `brand-{50..950}` for accent. Dark theme is default; light theme inverts the surface palette.

## Log Files

- **Windows:** `%APPDATA%\dev.leaguetoolkit.manager\logs\ltk-manager.log`
- **Linux/macOS:** `~/.local/share/dev.leaguetoolkit.manager/logs/ltk-manager.log`
