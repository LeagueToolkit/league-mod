# LTK Manager - Application Design Document

This document provides a high-level overview of the LTK Manager application, its architecture, features, and user flows.

---

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [User Personas](#user-personas)
- [Features](#features)
  - [Mod Library](#1-mod-library)
  - [Settings](#2-settings)
  - [Mod Inspector](#3-mod-inspector)
  - [Creator Workshop](#4-creator-workshop)
  - [First-Run Experience](#5-first-run-experience)
- [User Flows](#user-flows)
- [Data Model](#data-model)
- [Security Model](#security-model)
- [Future Roadmap](#future-roadmap)

---

## Overview

LTK Manager is a desktop application for managing League of Legends mods built on the `.modpkg` format. It serves as the graphical counterpart to the `league-mod` CLI tool, making mod management accessible to users who prefer visual interfaces.

### Goals

| Goal | Description |
|------|-------------|
| **Accessibility** | Make mod management approachable for non-technical users |
| **Type Safety** | Leverage Rust and TypeScript for compile-time guarantees |
| **Performance** | Fast startup, minimal memory footprint, responsive UI |
| **Integration** | Seamlessly work with the LeagueToolkit ecosystem |

### Tech Stack

| Layer | Technology | Purpose |
|-------|------------|---------|
| Runtime | Tauri v2 | Native desktop application framework |
| Backend | Rust | Core logic, file operations, mod processing |
| Frontend | React 19 | User interface |
| Routing | TanStack Router | Type-safe navigation |
| Styling | Tailwind CSS v4 | Visual design system |
| State | Zustand | Client-side state management |

---

## Architecture

The application follows a two-layer architecture with clear separation of concerns:

### Frontend Layer (WebView)

The frontend is responsible for:
- Rendering the user interface
- Handling user interactions
- Managing client-side UI state
- Communicating with the backend via IPC

### Backend Layer (Rust)

The backend is responsible for:
- File system operations (reading/writing mods)
- Mod package processing (using `ltk_modpkg` library)
- Project configuration (using `ltk_mod_project` library)
- Settings persistence
- League of Legends installation detection

### Communication

Frontend and backend communicate through Tauri's IPC (Inter-Process Communication) system. The frontend invokes commands on the backend, which processes them and returns results. All data is serialized as JSON during transit.

---

## User Personas

### Mod Consumer

**Description**: A League of Legends player who wants to customize their game with community-made mods.

**Needs**:
- Easy mod installation (drag & drop)
- Simple enable/disable toggles
- Visual mod library management
- Automatic League installation detection

**Technical Skill**: Low to medium

### Mod Creator

**Description**: A content creator who builds mods for the League community.

**Needs**:
- Project creation wizard
- Metadata editing tools
- Layer management
- One-click packaging to `.modpkg`
- Quick testing workflow

**Technical Skill**: Medium to high

---

## Features

### 1. Mod Library

The central hub for managing installed mods.

**Capabilities**:
- View all installed mods in grid or list layout
- Search and filter mods by name, author, or tags
- Enable/disable individual mods with a toggle
- View detailed mod information (metadata, layers, size)
- Uninstall mods with confirmation
- Drag & drop installation of `.modpkg` files

**Mod Card Information**:
- Thumbnail image
- Display name and version
- Author(s)
- Enable/disable toggle
- Quick actions menu

---

### 2. Settings

Application configuration and preferences.

**Settings Categories**:

| Category | Options |
|----------|---------|
| **League Path** | Auto-detect or manually browse to League installation |
| **Mod Storage** | Location where installed mods are stored |
| **Workshop Path** | Directory where mod projects are stored and managed |
| **Appearance** | Theme selection (Light, Dark, System) |
| **About** | App version, links to documentation and support |

---

### 3. Mod Inspector

Preview and inspect `.modpkg` files before installation.

**Information Displayed**:
- Mod metadata (name, version, description)
- Author information
- Available layers with descriptions
- File count and total size
- Digital signature status (if signed)

**Actions**:
- Install the mod
- Extract to folder for inspection

---

### 4. Creator Workshop

A comprehensive workspace for mod creators to build, manage, and package mods.

#### 4.1 Project Browser

The main view of the Creator Workshop, displaying all mod projects in the configured Workshop directory.

**Project Discovery**:
- Scans the Workshop directory for folders containing `mod.config.json` or `mod.config.toml`
- Reads and parses project configuration using `ltk_mod_project`
- Displays project metadata in a browsable list/grid

**Project Card Information**:
- Project name and display name
- Version number
- Last modified date
- Layer count
- Quick actions (Open, Pack, Delete)

**Project Management**:
- Create new project (opens wizard)
- Import existing project (copy/move to Workshop directory)
- Open project folder in file explorer
- Remove project from Workshop
- Refresh project list

#### 4.2 New Project Wizard

Step-by-step guided creation of a new mod project.

**Wizard Steps**:

| Step | Fields |
|------|--------|
| **1. Basics** | Project name, display name, initial version |
| **2. Details** | Description, authors, license selection |
| **3. Template** | Select from templates or start blank |
| **4. Layers** | Configure initial layer structure |
| **5. Confirm** | Review and create project |

**Project Templates**:

| Template | Description | Pre-configured For |
|----------|-------------|-------------------|
| **Blank** | Empty project structure | Any mod type |
| **Champion Skin** | Skin modification setup | Character textures, models |
| **Map Mod** | Map modification setup | Summoner's Rift assets |
| **UI Mod** | Interface modification | HUD elements, fonts |
| **Sound Mod** | Audio replacement | SFX, music, voice |

#### 4.3 Project Editor

Full-featured editor for modifying project configuration and content.

**Metadata Panel**:
- Edit name, display name, version
- Manage description (supports markdown preview)
- Author management (add, remove, edit roles)
- License selection from common options or custom
- Thumbnail selection and preview

**Layer Manager**:
- View all layers in priority order
- Add new layers with name and description
- Remove layers (with content warning)
- Reorder layers via drag & drop
- Set layer priority values
- Mark layers as optional or required

**Content Browser**:
- Tree view of project content directory
- Organized by layer
- File/folder operations (create, rename, delete)
- Open files in external editor
- Show file sizes and counts per layer

**Transformer Configuration**:
- View configured file transformers
- Add/remove transformer rules
- Configure glob patterns for file matching
- Set transformer-specific options

#### 4.4 Build System

Tools for validating and packaging mod projects.

**Pre-Build Validation**:
- Check for missing required files
- Validate configuration syntax
- Warn about empty layers
- Check for path conflicts between layers
- Verify transformer configurations

**Build Process**:
- One-click pack to `.modpkg`
- Progress indicator with current operation
- Configurable output location
- Option to auto-increment version on build

**Build Output**:
- Success/failure status with details
- Output file location with "Open Folder" action
- File size of generated package
- Build duration

**Build History**:
- List of recent builds for each project
- Timestamp and version for each build
- Success/failure status
- Quick access to output files

#### 4.5 Quick Test Workflow

Streamlined testing of mods during development.

**Test Actions**:
- Pack and install to library in one click
- Launch League with mod enabled
- Revert to clean state after testing

**Test Configuration**:
- Select which layers to include in test build
- Option to skip validation for faster iteration
- Auto-uninstall previous test builds

#### 4.6 Project Import/Export

Tools for sharing and backing up projects.

**Import Sources**:
- From existing folder (copy or link)
- From `.modpkg` file (extract and create project)
- From Git repository URL (clone)

**Export Options**:
- Export as ZIP archive
- Export to different location
- Include/exclude build outputs

---

### 5. First-Run Experience

Guided onboarding for new users.

**Steps**:
1. Welcome screen with app introduction
2. League of Legends path detection/selection
3. Workshop directory selection (for creators)
4. Brief feature tour
5. Ready to use confirmation

---

## User Flows

### Installing a Mod

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Drag .modpkg onto window    →    Show drop zone highlight
2. Drop file                   →    Validate file format
3. [If valid]                  →    Show mod preview dialog
4. Click "Install"             →    Copy to mod storage
5. [Success]                   →    Add to library, show toast
6. [Error]                     →    Show error message
```

### Enabling/Disabling a Mod

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Click toggle on mod card    →    Optimistic UI update
2. [Background]                →    Update mod state in backend
3. [Success]                   →    Confirm state persisted
4. [Error]                     →    Revert UI, show error
```

### Configuring League Path

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Open Settings               →    Show current path (if any)
2. Click "Auto-detect"         →    Scan common locations
3. [If found]                  →    Validate and display path
4. [If not found]              →    Prompt manual selection
5. Click "Browse"              →    Open folder picker
6. Select folder               →    Validate League installation
7. [If valid]                  →    Save and show success
8. [If invalid]                →    Show validation error
```

### Creating a Mod Project

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Click "New Project"         →    Open wizard dialog
2. Enter basic info            →    Validate name (no conflicts)
3. Select template             →    Preview template structure
4. Configure layers            →    Validate layer names
5. Click "Create"              →    Generate project in Workshop
6. [Success]                   →    Open project in editor
7. [Error]                     →    Show error, allow retry
```

### Opening an Existing Project

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Navigate to Workshop        →    Scan Workshop directory
2. [Projects found]            →    Display project cards
3. Click on project            →    Load project configuration
4. [Config valid]              →    Open project editor
5. [Config invalid]            →    Show error with details
```

### Building a Mod Package

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Click "Pack" in editor      →    Run pre-build validation
2. [Validation passes]         →    Show build progress
3. [During build]              →    Update progress indicator
4. [Build complete]            →    Show success with file path
5. [Build failed]              →    Show error details
6. [Optional] Click "Open"     →    Open output folder
```

### Quick Test Workflow

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Click "Quick Test"          →    Build mod (skip full validation)
2. [Build success]             →    Install to library (test mode)
3. [Auto]                      →    Enable mod, disable others
4. Click "Launch League"       →    Start League of Legends
5. [After testing]             →    Click "End Test"
6. [Cleanup]                   →    Uninstall test build, restore state
```

### Importing a Project from .modpkg

```
User Action                          System Response
─────────────────────────────────────────────────────────────
1. Click "Import"              →    Show import options
2. Select "From .modpkg"       →    Open file picker
3. Select file                 →    Extract and analyze contents
4. Enter project name          →    Validate name availability
5. Click "Import"              →    Create project in Workshop
6. [Success]                   →    Open project in editor
```

---

## Data Model

### Settings

| Field | Type | Description |
|-------|------|-------------|
| League Path | Path (optional) | Path to League of Legends installation |
| Mod Storage Path | Path (optional) | Where installed mods are stored |
| Workshop Path | Path (optional) | Directory for mod projects |
| Theme | Enum | Light, Dark, or System |
| First Run Complete | Boolean | Whether onboarding has been completed |

### Installed Mod

| Field | Type | Description |
|-------|------|-------------|
| ID | UUID | Unique identifier |
| Name | String | Internal name (slug format) |
| Display Name | String | Human-readable name |
| Version | String | Semantic version (e.g., "1.2.0") |
| Description | String (optional) | Mod description |
| Authors | List of strings | Creator names |
| Enabled | Boolean | Whether mod is active |
| Installed At | Timestamp | When mod was installed |
| File Path | Path | Location of .modpkg file |
| Layers | List | Available mod layers |

### Mod Layer

| Field | Type | Description |
|-------|------|-------------|
| Name | String | Layer identifier |
| Description | String | Layer description |
| Priority | Integer | Load order priority |
| Enabled | Boolean | Whether layer is active |

### Mod Project (Workshop)

| Field | Type | Description |
|-------|------|-------------|
| Path | Path | Absolute path to project directory |
| Name | String | Internal project name |
| Display Name | String | Human-readable name |
| Version | String | Current version |
| Description | String (optional) | Project description |
| Authors | List | Author information with roles |
| License | String (optional) | License identifier |
| Layers | List | Configured layers |
| Transformers | List | File transformer configurations |
| Last Modified | Timestamp | When config was last changed |

### Project Author

| Field | Type | Description |
|-------|------|-------------|
| Name | String | Author's name |
| Role | String (optional) | Role in project (e.g., "Lead", "Contributor") |
| URL | String (optional) | Author's website or profile |

### Project Layer

| Field | Type | Description |
|-------|------|-------------|
| Name | String | Layer identifier |
| Description | String (optional) | What this layer contains |
| Priority | Integer | Load order (higher = loaded later) |

### Build Record

| Field | Type | Description |
|-------|------|-------------|
| Project Name | String | Which project was built |
| Version | String | Version that was built |
| Timestamp | Timestamp | When build occurred |
| Success | Boolean | Whether build succeeded |
| Output Path | Path (optional) | Location of output file |
| Duration | Duration | How long build took |
| Error | String (optional) | Error message if failed |

---

## Security Model

### Permissions

The application requests only necessary permissions:

| Permission | Purpose |
|------------|---------|
| File System (Scoped) | Read/write mod files, projects, and settings |
| Dialog | File picker and folder selection |
| Shell (Limited) | Open URLs in browser, open folders in explorer |

### Content Security

- WebView content is loaded from local files only
- No arbitrary code execution from mod files
- Settings and mod index stored in app data directory

### Mod Safety

- Mods are sandboxed within the mod storage directory
- Projects are sandboxed within the Workshop directory
- Digital signatures can verify mod authenticity (when signed)
- No automatic execution of mod contents

---

## Future Roadmap

### Version 1.0 (MVP)

- [x] Basic mod library with grid view
- [x] Drag & drop mod installation
- [x] Enable/disable toggles
- [x] Settings page with League path detection
- [ ] Mod uninstallation
- [ ] Settings persistence

### Version 1.1

- [ ] Mod inspector for previewing before install
- [ ] List view for mod library
- [ ] Search and filtering
- [ ] Layer selection per mod

### Version 1.2 (Creator Workshop)

- [ ] Workshop directory configuration
- [ ] Project browser with auto-discovery
- [ ] New project wizard with templates
- [ ] Project metadata editor
- [ ] Layer manager
- [ ] Pack to .modpkg with validation

### Version 1.3 (Workshop Enhanced)

- [ ] Content browser for project files
- [ ] Build history
- [ ] Quick test workflow
- [ ] Project import from .modpkg
- [ ] Transformer configuration UI

### Version 2.0

- [ ] Mod profiles (save/load configurations)
- [ ] Conflict detection between mods
- [ ] Mod browser (discover community mods)
- [ ] Auto-update checking for mods
- [ ] Cloud sync for mod library

---

## Appendix

### Glossary

| Term | Definition |
|------|------------|
| **Modpkg** | The `.modpkg` file format for distributing League mods |
| **Layer** | A subset of mod content that can be independently enabled |
| **Workshop** | The directory where mod projects are stored and managed |
| **Transformer** | A processing step applied to files during packaging |
| **IPC** | Inter-Process Communication between frontend and backend |
| **Tauri** | Framework for building native desktop apps with web UI |

### Related Documentation

- [league-mod CLI documentation](../league-mod/README.md)
- [ltk_modpkg library](../ltk_modpkg/README.md)
- [ltk_mod_project library](../ltk_mod_project/README.md)
