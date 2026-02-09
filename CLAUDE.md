# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

League Mod Toolkit - A Rust workspace containing CLI tools and a desktop app (Tauri + React) for creating, managing, and distributing League of Legends mods using the `.modpkg` format.

## Documentation

- **Error Handling**: See [crates/ltk-manager/docs/ERROR_HANDLING.md](crates/ltk-manager/docs/ERROR_HANDLING.md) for detailed explanation of the IpcResult pattern and common mistakes.

## Quick Commands

### Rust (CLI and Libraries)

```bash
# Build all crates
cargo build --release

# Run CLI
cargo run --bin league-mod -- <command>

# Run tests
cargo test

# Run tests for specific crate
cargo test -p ltk_modpkg

# Lint
cargo clippy

# Format
cargo fmt
```

### LTK Manager (Desktop GUI)

```bash
cd crates/ltk-manager

# Install dependencies
pnpm install

# Development mode (hot reload)
pnpm tauri dev

# Frontend only (no Rust rebuild)
pnpm dev

# Type check
pnpm typecheck

# Lint
pnpm lint

# Format
pnpm format

# Full check (typecheck + lint + format)
pnpm check

# Build for production
pnpm tauri build
```

## Architecture Overview

### Workspace Structure

This is a Cargo workspace with the following crates:

- **`league-mod`** - CLI tool for mod developers (init, pack, extract, info)
- **`ltk_modpkg`** - Binary format library for `.modpkg` files (reading, writing, compression)
- **`ltk_mod_project`** - Configuration library (JSON/TOML config, metadata structures)
- **`ltk_mod_core`** - Shared utilities (League path detection, cross-platform utilities)
- **`ltk_fantome`** - Fantome archive format support (`.fantome` files)
- **`ltk_overlay`** - Overlay building engine (WAD patching, game file indexing)
- **`ltk_pki`** - Public Key Infrastructure (mod signing/verification)
- **`ltk-manager`** - Desktop GUI application (Tauri + React)

### LTK Manager Architecture

Two-layer architecture with clear separation:

**Frontend (React + TypeScript):**
- TanStack Router for type-safe file-based routing
- TanStack Query for all server state management
- Tailwind CSS v4 for styling
- Module-based organization (`library`, `patcher`, `settings`, `workshop`)

**Backend (Rust + Tauri):**
- Commands layer (`commands/*.rs`) - IPC entry points
- Business logic (`mods/`, `overlay/`, `patcher/`) - Core functionality
- External crates (`ltk_overlay`, `ltk_modpkg`, `ltk_fantome`) - Specialized operations

**Communication:**
- Tauri IPC with Result-based error handling
- Commands return `IpcResult<T>` (serializable `Ok { value }` or `Err { error }`)
- Frontend uses `Result<T, E>` pattern throughout

## Key Architectural Patterns

### Backend (Rust)

**Storage System (Archive-Based):**
```
{modStoragePath}/
├── library.json           # Index: mods + profiles + active_profile_id
├── archives/              # Stored .modpkg/.fantome files
│   └── {mod_id}.modpkg
├── metadata/              # Extracted metadata only (for UI)
│   └── {mod_id}/
│       ├── mod.config.json
│       └── thumbnail.webp
└── profiles/              # Profile-specific overlay and cache
    └── {profile_id}/
        ├── overlay/       # Built WAD overlays
        └── cache/         # Extracted mod content (temporary)
```

**Profile System:**
- Each profile has independent `enabled_mods` list
- Mods are shared across profiles (stored once in `archives/`)
- Profile switching changes which mods are active
- Each profile has isolated overlay and cache directories
- Default profile always exists and cannot be deleted

**Overlay Building Flow:**
1. Get active profile's `enabled_mods` from `library.json`
2. Extract enabled mods to profile-specific `cache/` directory
3. Create `OverlayBuilder` with game path and profile's overlay directory
4. Builder indexes League files, collects mod overrides, patches WADs
5. Emits progress events: `indexing` → `collecting` → `patching` → `strings` → `complete`
6. Returns overlay path for legacy patcher DLL

**Command Pattern:**
```rust
// Three-layer command architecture
#[tauri::command]
pub fn command_name(args, app: AppHandle, state: State<T>) -> IpcResult<R> {
    command_name_inner(args, &app, &state).into()
}

fn command_name_inner(args, app: &AppHandle, state: &State<T>) -> AppResult<R> {
    let data = state.0.lock().mutex_err()?.clone();
    // business logic
}
```

**Error Handling:**
- Internal: `AppResult<T> = Result<T, AppError>`
- IPC: `IpcResult<T> = { Ok { value }, Err { error } }`
- Errors include code (SCREAMING_SNAKE_CASE) and context (JSON)

### Frontend (React)

**Module Organization:**
```
modules/{module}/
  api/
    index.ts          # Barrel exports
    keys.ts           # Query key factory (hierarchical)
    queries.ts        # Query options & hooks
    use*.ts           # Individual mutation/query hooks
  components/
    index.ts
    *.tsx
```

**Query Key Factory Pattern:**
```ts
libraryKeys = {
  all: ["library"],
  mods: () => [...libraryKeys.all, "mods"],
  mod: (id) => [...libraryKeys.mods(), id],
  profiles: () => [...libraryKeys.all, "profiles"],
  activeProfile: () => [...libraryKeys.profiles(), "active"]
}
```

**Query Options Pattern (Reusable):**
```ts
export function profilesQueryOptions() {
  return queryOptions<Profile[], AppError>({
    queryKey: libraryKeys.profiles(),
    queryFn: queryFn(api.listModProfiles),
  });
}

export function useProfiles() {
  return useQuery(profilesQueryOptions());
}
```

**Mutation Patterns:**
1. **Optimistic with Rollback** - Critical operations (toggle mod, switch profile)
2. **Cache Update on Success** - Simple operations (install mod)
3. **Invalidation** - Non-critical or complex data (create profile)

**Component Organization:**
- **Global components** (`src/components/`) - Reusable across app (Button, Toast, FormField)
- **Module components** (`src/modules/{module}/components/`) - Feature-specific
- Break complex components into sub-components in folders (e.g., ProfileSelector/)

## Critical Development Patterns

### Path Handling with Camino
**ALWAYS use `camino::Utf8Path` / `Utf8PathBuf` instead of `std::path::Path` / `PathBuf` for path handling in Rust code.** Camino provides UTF-8 guaranteed paths that are more robust, ergonomic, and consistent across platforms.

The workspace defines a shared version in the root `Cargo.toml` (currently `camino = "1.1"`). Prefer `camino = { workspace = true }` in crate `Cargo.toml` files.

**Key patterns:**
```rust
use camino::{Utf8Path, Utf8PathBuf};

// Function parameters: use &Utf8Path
fn process_file(path: &Utf8Path) -> Result<()> { ... }

// Owned paths: use Utf8PathBuf
struct Config {
    league_path: Option<Utf8PathBuf>,
}

// Construction
let path = Utf8PathBuf::from("some/path");
let joined = path.join("subdir");

// Converting FROM std::path (e.g., from OS APIs)
let std_path: PathBuf = std::env::current_exe()?;
let utf8_path = Utf8PathBuf::from_path_buf(std_path)
    .map_err(|p| format!("Non-UTF-8 path: {}", p.display()))?;

// Converting TO std::path (e.g., for std::fs APIs)
std::fs::File::open(utf8_path.as_std_path())?;
std::fs::read_dir(utf8_path.as_std_path())?;

// Direct string access (no lossy conversion needed)
println!("Path: {}", utf8_path.as_str());
```

**When to use `as_std_path()`:** At FFI boundaries where `std::fs` or external crates require `&Path` / `PathBuf`. Keep camino types throughout internal logic and convert only at the edges.

**Feature flags:** Add `serde1` feature when paths need serialization (e.g., in config structs):
```toml
camino = { workspace = true, features = ["serde1"] }
```

### Input Validation
**ALWAYS validate on backend, NEVER rely solely on frontend validation.**
- Trim and validate string inputs
- Check for empty/whitespace strings
- Return descriptive errors from backend

### Using Component Library
**ALWAYS use reusable components from `@/components` instead of native HTML or raw base-ui imports.** Never import from `@base-ui-components/react` directly in module code — all base-ui primitives must be wrapped in `src/components/` first and imported via `@/components`.

**Available wrapped components:**
- `Button` / `IconButton` - Variants: filled, light, outline, ghost, transparent; Sizes: xs, sm, md, lg, xl
- `Field` / `FormField` / `TextareaField` - Styled form inputs
- `Checkbox` / `CheckboxGroup` - Boolean/multi-select inputs
- `RadioGroup` - Mutually exclusive choices (compound: Root, Label, Options, Card, Item)
- `Tabs` - Tabbed content (compound: Root, List, Tab, Panel, Indicator)
- `Tooltip` / `SimpleTooltip` - Hover information
- `Toast` / `useToast()` - Notifications
- `Dialog` - Modal dialogs (compound: Root, Portal, Backdrop, Overlay, Header, Title, Body, Footer, Close)
- `Switch` - Toggle on/off; Sizes: sm, md
- `Menu` - Dropdown menus (compound: Root, Trigger, Portal, Positioner, Popup, Item, Separator, Group, GroupLabel). Item supports `icon` and `variant="danger"`.
- `Select` / `SelectField` - Dropdown select inputs. Compound for custom layouts, `SelectField` for quick use with `options` array. TanStack Form: `field.SelectField`.
- `Popover` - Positioned popover panels (compound: Root, Trigger, Portal, Backdrop, Positioner, Popup, Arrow, Title, Description, Close).

**Not yet wrapped (create in `src/components/` before using):** AlertDialog, Separator, Progress, ScrollArea

### Adding Tauri Commands

**CRITICAL**: All Tauri commands MUST return `IpcResult<T>`, never plain `Result<T, E>`.

Commands that return plain `Result` will serialize as `null`, causing frontend crashes with "Cannot read properties of null (reading 'ok')".

**Standard Pattern:**
```rust
use crate::error::{AppResult, IpcResult};

#[tauri::command]
pub fn my_command(args: String) -> IpcResult<ReturnType> {
    my_command_inner(&args).into()
}

fn my_command_inner(args: &str) -> AppResult<ReturnType> {
    // Business logic here using Result<T, AppError>
    Ok(value)
}
```

**Steps:**
1. Implement function in backend module (e.g., `mods/mod.rs`)
2. Create command wrapper in `commands/` with `#[tauri::command]` that returns `IpcResult<T>`
3. Register in `main.rs` with `tauri::generate_handler![]`
4. Add TypeScript types in `src/lib/tauri.ts`
5. Create React Query hooks in `src/modules/.../api/`

### Query Patterns
- Use `queryOptions()` for reusability and type safety
- Use query key factory for hierarchical invalidation
- Barrel export all API functions through module's `index.ts`
- Use `queryFn()` and `mutationFn()` helpers from `utils/query.ts`

## Mod Format Reference

### Project Structure
```
my-mod/
├── mod.config.json           # Project configuration
├── content/                  # Mod content by layer
│   ├── base/                 # Base layer (priority 0)
│   │   ├── Aatrox.wad.client # Files for Aatrox WAD
│   │   └── Map11.wad.client  # Files for Summoner's Rift
│   └── high_res/             # Optional layer
└── build/                    # Output .modpkg files
```

### Layer System
- Layers have priorities (higher = loaded later)
- Higher priority layers override lower priority layers
- Base layer always present (priority 0)
- Additional layers are optional

## CI/CD

All contributions go through CI:
- Code compilation (Linux, Windows, macOS)
- Test suite execution
- Clippy linting
- Format verification
- Security audit
- License checks

**Commit Message Format:**
This project uses [Conventional Commits](https://www.conventionalcommits.org/):
```bash
feat: add support for custom transformers      # Minor version bump
fix: resolve file path handling on Windows     # Patch version bump
feat!: change configuration file format        # Major version bump (breaking)
docs: update installation instructions         # No version bump
```

## MCP Setup (TanStack)

Create `.mcp.json` in project root (gitignored):

**Windows:**
```json
{
  "mcpServers": {
    "tanstack": {
      "command": "cmd",
      "args": ["/c", "npx", "@tanstack/cli", "mcp"]
    }
  }
}
```

**macOS/Linux:**
```json
{
  "mcpServers": {
    "tanstack": {
      "command": "npx",
      "args": ["@tanstack/cli", "mcp"]
    }
  }
}
```

Restart Claude Code after creating the file.

## Important Constraints

### Profile System
- Default profile cannot be deleted or renamed
- Cannot switch profiles while patcher is running
- Each profile has independent overlay and cache directories

### Mod Safety
- Mods stored as archives, not extracted
- Only metadata extracted for UI display
- Full content extracted to cache on-demand during overlay build
- Archives deleted on uninstall

### League Detection
- Uses `ltk_mod_core::auto_detect_league_path()` for registry/process/common path detection
- Auto-detection runs on first startup if path not configured
- Path stored in `{appData}/settings.json`

## Log Files

Backend logs available at:
- **Windows:** `%APPDATA%\dev.leaguetoolkit.manager\logs\ltk-manager.log`
- **Linux/macOS:** `~/.local/share/dev.leaguetoolkit.manager/logs/ltk-manager.log`

Control verbosity with `RUST_LOG` environment variable:
```bash
RUST_LOG=ltk_manager=trace,tauri=info pnpm tauri dev
```
