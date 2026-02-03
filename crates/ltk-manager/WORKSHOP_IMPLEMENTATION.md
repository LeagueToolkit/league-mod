# Workshop Implementation Plan

This document outlines the implementation plan for the Creator Workshop feature in LTK Manager. The workshop provides a graphical interface for mod creators to manage projects, mirroring the `league-mod` CLI workflow.

## Table of Contents

- [Overview](#overview)
- [Scope](#scope)
- [Architecture](#architecture)
- [Phase 1: Backend Commands](#phase-1-backend-commands)
- [Phase 2: Frontend API Layer](#phase-2-frontend-api-layer)
- [Phase 3: UI Components](#phase-3-ui-components)
- [Phase 4: Workshop Page](#phase-4-workshop-page)
- [Phase 5: Settings Integration](#phase-5-settings-integration)
- [Data Types](#data-types)
- [File Reference](#file-reference)
- [Testing & Verification](#testing--verification)

---

## Overview

The LTK Manager already has infrastructure in place:

| Component        | Status                               | Location                                    |
| ---------------- | ------------------------------------ | ------------------------------------------- |
| Route            | Placeholder "Coming Soon"            | `src/routes/creator.tsx`                    |
| Module directory | Empty, ready for implementation      | `src/modules/workshop/`                     |
| Settings field   | `workshopPath` configured but unused | `src-tauri/src/state.rs`                    |
| Navigation       | "Workshop" link in TitleBar          | `src/modules/shell/components/TitleBar.tsx` |
| Design spec      | Comprehensive (sections 4.1-4.6)     | `DESIGN.md`                                 |

---

## Scope

**Core MVP (Version 1.2)** - Features to implement:

| Feature            | Description                                                       |
| ------------------ | ----------------------------------------------------------------- |
| Project Browser    | List/grid view of projects in workshop directory                  |
| New Project Wizard | Multi-step creation with name, display name, description, authors |
| Metadata Editor    | View and edit project configuration                               |
| Layer Manager      | Add, remove, reorder layers                                       |
| Pack to .modpkg    | Build distributable packages with validation                      |

**Deferred to Version 1.3:**

- Content browser (file tree)
- Build history
- Quick test workflow
- Import from .modpkg
- Transformer configuration UI

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Frontend (React)                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  Workshop   │  │   Project   │  │   New Project       │  │
│  │  Page       │  │   Editor    │  │   Wizard            │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                     │             │
│  ┌──────┴────────────────┴─────────────────────┴──────────┐ │
│  │                    TanStack Query                       │ │
│  │    useWorkshopProjects, useCreateProject, usePackProject│ │
│  └──────────────────────────┬──────────────────────────────┘ │
│                             │ IPC (invoke)                   │
└─────────────────────────────┼───────────────────────────────┘
                              │
┌─────────────────────────────┼───────────────────────────────┐
│                      Backend (Rust)                          │
│  ┌──────────────────────────┴──────────────────────────────┐│
│  │                 Tauri Commands                          ││
│  │  get_workshop_projects, create_project, pack_project    ││
│  └──────────────────────────┬──────────────────────────────┘│
│                             │                                │
│  ┌──────────────────────────┴──────────────────────────────┐│
│  │              ltk_mod_project + ltk_modpkg               ││
│  │     (Project parsing, validation, package building)     ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Backend Commands

### 1.1 Error Types

**File:** `src-tauri/src/error.rs`

Add new error variants:

```rust
// Add to ErrorCode enum
WorkshopNotConfigured,
ProjectNotFound,
ProjectAlreadyExists,
InvalidProjectConfig,
PackFailed,

// Add to AppError enum
#[error("Workshop directory not configured")]
WorkshopNotConfigured,

#[error("Project not found: {0}")]
ProjectNotFound(String),

#[error("Project already exists: {0}")]
ProjectAlreadyExists(String),

#[error("Invalid project configuration: {0}")]
InvalidProjectConfig(String),

#[error("Pack failed: {0}")]
PackFailed(String),
```

### 1.2 Workshop Commands

**New file:** `src-tauri/src/commands/workshop.rs`

Follow the pattern from `mods.rs`:

- Public `#[tauri::command]` function calls private `_inner` function
- Use `IpcResult<T>` for returns, converting via `.into()`
- Use `State<SettingsState>` for settings access

| Command                   | Signature                                                        | Description                          |
| ------------------------- | ---------------------------------------------------------------- | ------------------------------------ |
| `get_workshop_projects`   | `() -> IpcResult<Vec<WorkshopProject>>`                          | Scan workshop directory for projects |
| `get_workshop_project`    | `(project_path: String) -> IpcResult<WorkshopProject>`           | Load single project                  |
| `create_workshop_project` | `(args: CreateProjectArgs) -> IpcResult<WorkshopProject>`        | Create new project                   |
| `save_project_config`     | `(project_path: String, config: ProjectConfig) -> IpcResult<()>` | Save metadata                        |
| `delete_workshop_project` | `(project_path: String) -> IpcResult<()>`                        | Delete project folder                |
| `pack_workshop_project`   | `(args: PackProjectArgs) -> IpcResult<PackResult>`               | Build .modpkg                        |
| `validate_project`        | `(project_path: String) -> IpcResult<Vec<ValidationIssue>>`      | Pre-build validation                 |

#### Implementation Notes

**Project Discovery (`get_workshop_projects`):**

```rust
// Scan workshop_path for directories containing mod.config.json or mod.config.toml
// Use ltk_mod_project::ModProject::from_file() to parse
// Return list with path, metadata, and lastModified timestamp
```

**Project Creation (`create_workshop_project`):**

```rust
// 1. Validate name is slug-format (alphanumeric + hyphens)
// 2. Check project doesn't already exist
// 3. Create directory structure:
//    {workshop_path}/{name}/
//    ├── mod.config.json
//    └── content/
//        └── base/
// 4. Write initial config using serde_json
// 5. Return created project
```

**Packing (`pack_workshop_project`):**

```rust
// 1. Load project config
// 2. Run validation
// 3. Use ltk_modpkg::project::pack_project() or similar
// 4. Return PackResult with output_path, file_size, duration_ms
```

### 1.3 Register Commands

**File:** `src-tauri/src/commands/mod.rs`

```rust
mod workshop;
pub use workshop::*;
```

**File:** `src-tauri/src/main.rs`

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    // Workshop
    commands::get_workshop_projects,
    commands::get_workshop_project,
    commands::create_workshop_project,
    commands::save_project_config,
    commands::delete_workshop_project,
    commands::pack_workshop_project,
    commands::validate_project,
])
```

---

## Phase 2: Frontend API Layer

### 2.1 TypeScript Types

**File:** `src/lib/tauri.ts`

```typescript
// Workshop types
export interface WorkshopProject {
  path: string;
  name: string;
  displayName: string;
  version: string;
  description: string;
  authors: ProjectAuthor[];
  layers: ProjectLayer[];
  thumbnailPath?: string;
  lastModified: string; // ISO timestamp
}

export interface ProjectAuthor {
  name: string;
  role?: string;
}

export interface ProjectLayer {
  name: string;
  priority: number;
  description?: string;
}

export interface CreateProjectArgs {
  name: string;
  displayName: string;
  description: string;
  authors: string[];
}

export interface PackProjectArgs {
  projectPath: string;
  outputDir?: string;
  format: "modpkg" | "fantome";
  fileName?: string;
}

export interface PackResult {
  outputPath: string;
  fileSize: number;
  durationMs: number;
}

export interface ValidationIssue {
  severity: "error" | "warning";
  message: string;
  path?: string;
}

// Add to api object
export const api = {
  // ... existing ...

  // Workshop
  getWorkshopProjects: () => invokeResult<WorkshopProject[]>("get_workshop_projects"),
  getWorkshopProject: (projectPath: string) =>
    invokeResult<WorkshopProject>("get_workshop_project", { projectPath }),
  createWorkshopProject: (args: CreateProjectArgs) =>
    invokeResult<WorkshopProject>("create_workshop_project", { args }),
  saveProjectConfig: (projectPath: string, config: ProjectConfig) =>
    invokeResult<void>("save_project_config", { projectPath, config }),
  deleteWorkshopProject: (projectPath: string) =>
    invokeResult<void>("delete_workshop_project", { projectPath }),
  packWorkshopProject: (args: PackProjectArgs) =>
    invokeResult<PackResult>("pack_workshop_project", { args }),
  validateProject: (projectPath: string) =>
    invokeResult<ValidationIssue[]>("validate_project", { projectPath }),
};
```

### 2.2 Query Key Factory

**New file:** `src/modules/workshop/api/keys.ts`

```typescript
export const workshopKeys = {
  all: ["workshop"] as const,
  projects: () => [...workshopKeys.all, "projects"] as const,
  project: (path: string) => [...workshopKeys.projects(), path] as const,
};
```

### 2.3 Query Hooks

**New file:** `src/modules/workshop/api/useWorkshopProjects.ts`

```typescript
import { useQuery } from "@tanstack/react-query";
import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { queryFn } from "@/utils/query";
import { workshopKeys } from "./keys";

export function useWorkshopProjects() {
  return useQuery<WorkshopProject[], AppError>({
    queryKey: workshopKeys.projects(),
    queryFn: queryFn(api.getWorkshopProjects),
  });
}
```

**New file:** `src/modules/workshop/api/useWorkshopProject.ts`

```typescript
import { useQuery } from "@tanstack/react-query";
import { api, type AppError, type WorkshopProject } from "@/lib/tauri";
import { queryFn } from "@/utils/query";
import { workshopKeys } from "./keys";

export function useWorkshopProject(projectPath: string) {
  return useQuery<WorkshopProject, AppError>({
    queryKey: workshopKeys.project(projectPath),
    queryFn: queryFn(() => api.getWorkshopProject(projectPath)),
    enabled: !!projectPath,
  });
}
```

### 2.4 Mutation Hooks

**New file:** `src/modules/workshop/api/useCreateProject.ts`

```typescript
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type CreateProjectArgs, type WorkshopProject } from "@/lib/tauri";
import { mutationFn } from "@/utils/query";
import { workshopKeys } from "./keys";

export function useCreateProject() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: mutationFn((args: CreateProjectArgs) => api.createWorkshopProject(args)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: workshopKeys.projects() });
    },
  });
}
```

**New file:** `src/modules/workshop/api/usePackProject.ts`

```typescript
import { useMutation } from "@tanstack/react-query";
import { api, type PackProjectArgs, type PackResult } from "@/lib/tauri";
import { mutationFn } from "@/utils/query";

export function usePackProject() {
  return useMutation({
    mutationFn: mutationFn((args: PackProjectArgs) => api.packWorkshopProject(args)),
  });
}
```

**New file:** `src/modules/workshop/api/useDeleteProject.ts`

```typescript
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/tauri";
import { mutationFn } from "@/utils/query";
import { workshopKeys } from "./keys";

export function useDeleteProject() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: mutationFn((projectPath: string) => api.deleteWorkshopProject(projectPath)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: workshopKeys.projects() });
    },
  });
}
```

### 2.5 Module Index

**New file:** `src/modules/workshop/api/index.ts`

```typescript
export { workshopKeys } from "./keys";
export { useWorkshopProjects } from "./useWorkshopProjects";
export { useWorkshopProject } from "./useWorkshopProject";
export { useCreateProject } from "./useCreateProject";
export { usePackProject } from "./usePackProject";
export { useDeleteProject } from "./useDeleteProject";
export { useSaveProjectConfig } from "./useSaveProjectConfig";
```

---

## Phase 3: UI Components

### 3.1 Project Card

**New file:** `src/modules/workshop/components/ProjectCard.tsx`

Similar to `ModCard.tsx` pattern:

```typescript
interface ProjectCardProps {
  project: WorkshopProject;
  viewMode: "grid" | "list";
  onOpen: () => void;
  onPack: () => void;
  onDelete: () => void;
}
```

**Display:**

- Thumbnail (or placeholder icon)
- Project name and version
- Author(s)
- Layer count badge
- Last modified date

**Actions (context menu):**

- Open in Editor
- Pack
- Open Folder
- Delete

### 3.2 New Project Dialog

**New file:** `src/modules/workshop/components/NewProjectDialog.tsx`

Multi-step wizard with Tabs component:

**Step 1 - Basics:**

- Name (slug format, validated)
- Display Name
- Version (default: "0.1.0")

**Step 2 - Details:**

- Description (textarea)
- Authors (add/remove list)

**Step 3 - Confirm:**

- Review all inputs
- "Create Project" button

### 3.3 Project Editor

**New file:** `src/modules/workshop/components/ProjectEditor.tsx`

Slide-over panel or modal with sections:

**Metadata Section:**

- Display name, version, description fields
- Save button

**Authors Section:**

- List with name and role
- Add/remove/edit buttons

**Layers Section:**

- Table: name, priority, description
- Add layer button
- Delete layer button (with warning for base)
- Reorder via drag-drop or priority input

### 3.4 Pack Dialog

**New file:** `src/modules/workshop/components/PackDialog.tsx`

**Options:**

- Format select: modpkg (recommended) / fantome
- Output directory (default: project/build)
- Custom filename (optional)

**States:**

- Idle: show form
- Validating: show validation progress
- Validation errors: show issues with fix suggestions
- Packing: show progress
- Success: show result with "Open Folder" button
- Error: show error message

### 3.5 Component Index

**New file:** `src/modules/workshop/components/index.ts`

```typescript
export { ProjectCard } from "./ProjectCard";
export { NewProjectDialog } from "./NewProjectDialog";
export { ProjectEditor } from "./ProjectEditor";
export { PackDialog } from "./PackDialog";
```

---

## Phase 4: Workshop Page

**Update file:** `src/routes/creator.tsx`

Replace placeholder with full workshop UI:

```typescript
function CreatorPage() {
  const { data: settings } = useSettings();
  const { data: projects, isLoading, error } = useWorkshopProjects();
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [search, setSearch] = useState("");
  const [showNewProjectDialog, setShowNewProjectDialog] = useState(false);
  const [selectedProject, setSelectedProject] = useState<string | null>(null);

  // Handle states:
  // 1. Workshop path not configured -> show prompt to configure in settings
  // 2. Loading -> show spinner
  // 3. Error -> show error message
  // 4. No projects -> show empty state with "Create your first project" CTA
  // 5. Projects -> show project grid/list

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-16 items-center gap-4 border-b border-surface-600 px-6">
        <h2 className="text-xl font-semibold text-surface-100">Workshop</h2>
        <div className="flex-1" />
        <SearchInput value={search} onChange={setSearch} />
        <ViewModeToggle value={viewMode} onChange={setViewMode} />
        <Button onClick={() => setShowNewProjectDialog(true)}>
          <LuPlus /> New Project
        </Button>
      </header>

      <main className="flex-1 overflow-auto p-6">
        {/* Render based on state */}
      </main>

      <NewProjectDialog
        open={showNewProjectDialog}
        onOpenChange={setShowNewProjectDialog}
      />

      {selectedProject && (
        <ProjectEditor
          projectPath={selectedProject}
          onClose={() => setSelectedProject(null)}
        />
      )}
    </div>
  );
}
```

---

## Phase 5: Settings Integration

**Update file:** `src/pages/Settings.tsx`

Add Workshop Directory section after "Mod Storage":

```tsx
{
  /* Workshop Path */
}
<section className="space-y-4">
  <h3 className="text-lg font-medium text-surface-100">Creator Workshop</h3>
  <div className="space-y-2">
    <label className="block text-sm font-medium text-surface-400">Workshop Directory</label>
    <div className="flex gap-2">
      <input
        type="text"
        value={settings.workshopPath || ""}
        readOnly
        placeholder="Not configured"
        className="flex-1 rounded-lg border border-surface-600 bg-surface-700 px-4 py-2.5 text-surface-200 placeholder:text-surface-500"
      />
      <IconButton
        icon={<LuFolderOpen className="h-5 w-5" />}
        variant="outline"
        size="lg"
        onClick={handleBrowseWorkshopPath}
      />
    </div>
    <p className="text-sm text-surface-500">Directory where your mod projects will be stored.</p>
  </div>
</section>;
```

---

## Data Types

### Rust Types

```rust
// src-tauri/src/workshop/mod.rs

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopProject {
    pub path: PathBuf,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<ProjectAuthor>,
    pub layers: Vec<ProjectLayer>,
    pub thumbnail_path: Option<PathBuf>,
    pub last_modified: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAuthor {
    pub name: String,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLayer {
    pub name: String,
    pub priority: i32,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectArgs {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub authors: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackProjectArgs {
    pub project_path: PathBuf,
    pub output_dir: Option<PathBuf>,
    pub format: PackFormat,
    pub file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackFormat {
    Modpkg,
    Fantome,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackResult {
    pub output_path: PathBuf,
    pub file_size: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
}
```

---

## File Reference

### Existing Files to Study

| File                                          | Purpose                                              |
| --------------------------------------------- | ---------------------------------------------------- |
| `src-tauri/src/commands/mods.rs`              | Pattern for Tauri commands (IpcResult, State access) |
| `src-tauri/src/mods/mod.rs`                   | Pattern for business logic separation                |
| `src/modules/library/api/useInstalledMods.ts` | Pattern for TanStack Query hooks                     |
| `src/modules/library/api/keys.ts`             | Pattern for query key factory                        |
| `src/pages/Library.tsx`                       | Pattern for list page with search, grid/list toggle  |
| `src/components/ModCard.tsx`                  | Pattern for card component                           |
| `crates/ltk_mod_project/src/lib.rs`           | ModProject struct for serialization compatibility    |
| `crates/league-mod/src/commands/pack.rs`      | CLI pack logic to reference                          |
| `crates/league-mod/src/commands/init.rs`      | CLI init logic to reference                          |

### New Files to Create

```
src-tauri/src/
├── commands/
│   └── workshop.rs          # NEW: Tauri commands
├── workshop/
│   └── mod.rs               # NEW: Business logic
└── error.rs                 # UPDATE: Add error types

src/
├── lib/
│   └── tauri.ts             # UPDATE: Add types and API
├── modules/workshop/
│   ├── api/
│   │   ├── index.ts         # NEW
│   │   ├── keys.ts          # NEW
│   │   ├── useWorkshopProjects.ts    # NEW
│   │   ├── useWorkshopProject.ts     # NEW
│   │   ├── useCreateProject.ts       # NEW
│   │   ├── useSaveProjectConfig.ts   # NEW
│   │   ├── useDeleteProject.ts       # NEW
│   │   └── usePackProject.ts         # NEW
│   ├── components/
│   │   ├── index.ts         # NEW
│   │   ├── ProjectCard.tsx  # NEW
│   │   ├── NewProjectDialog.tsx      # NEW
│   │   ├── ProjectEditor.tsx         # NEW
│   │   └── PackDialog.tsx   # NEW
│   └── index.ts             # UPDATE: Export all
├── routes/
│   └── creator.tsx          # UPDATE: Full implementation
├── pages/
│   └── Settings.tsx         # UPDATE: Add workshop path section
└── utils/
    └── errors.ts            # UPDATE: Add error codes
```

---

## Testing & Verification

### Backend Verification

```bash
# Check Rust compilation
cd crates/ltk-manager/src-tauri
cargo check

# Run tests (if any)
cargo test
```

### Frontend Verification

```bash
# Check TypeScript
cd crates/ltk-manager
pnpm tsc --noEmit

# Run dev server
pnpm tauri dev
```

### End-to-End Test Checklist

1. **Settings**
   - [ ] Configure workshop path in Settings
   - [ ] Path is persisted after app restart

2. **Project Browser**
   - [ ] Empty state shows when no projects exist
   - [ ] Projects display correctly in grid view
   - [ ] Projects display correctly in list view
   - [ ] Search filters projects by name

3. **Project Creation**
   - [ ] New Project dialog opens
   - [ ] Name validation (slug format) works
   - [ ] Project is created in workshop directory
   - [ ] Project appears in list after creation
   - [ ] `mod.config.json` is valid

4. **Project Editing**
   - [ ] Can open project editor
   - [ ] Can edit display name, version, description
   - [ ] Can add/remove authors
   - [ ] Can add/remove layers
   - [ ] Changes persist after save

5. **Packing**
   - [ ] Pack dialog opens
   - [ ] Validation runs before pack
   - [ ] Progress shows during pack
   - [ ] .modpkg file is created in build directory
   - [ ] Success message shows file size and location
   - [ ] "Open Folder" button works

6. **Error Handling**
   - [ ] Graceful error when workshop path not configured
   - [ ] Validation errors display clearly
   - [ ] Pack errors display with details

---

## Implementation Order

For incremental development, implement in this order:

1. **Backend foundation** (Phase 1.1-1.3)
   - Error types
   - `get_workshop_projects` command
   - Register command

2. **Frontend project list** (Phase 2 + 4 partial)
   - Types in tauri.ts
   - `useWorkshopProjects` hook
   - Basic workshop page with project grid
   - Empty states

3. **Project creation** (Phase 1 + 2 + 3 partial)
   - `create_workshop_project` command
   - `useCreateProject` hook
   - `NewProjectDialog` component

4. **Project editing** (Phase 1 + 2 + 3 partial)
   - `get_workshop_project`, `save_project_config` commands
   - Corresponding hooks
   - `ProjectEditor` component

5. **Packing** (Phase 1 + 2 + 3 partial)
   - `validate_project`, `pack_workshop_project` commands
   - Corresponding hooks
   - `PackDialog` component

6. **Settings** (Phase 5)
   - Workshop path configuration

7. **Polish**
   - Delete project functionality
   - Error handling refinements
   - Loading states and transitions
