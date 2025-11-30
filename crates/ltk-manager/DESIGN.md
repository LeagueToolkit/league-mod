# LTK Manager - Application Design Document

This document provides a high-level overview of the LTK Manager application, its architecture, features, and user flows.

---

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [User Personas](#user-personas)
- [Features](#features)
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

### 2. Settings

Application configuration and preferences.

**Settings Categories**:

| Category | Options |
|----------|---------|
| **League Path** | Auto-detect or manually browse to League installation |
| **Mod Storage** | Location where installed mods are stored |
| **Appearance** | Theme selection (Light, Dark, System) |
| **About** | App version, links to documentation and support |

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

### 4. Creator Workshop

Tools for mod creators to build and package mods.

**Planned Features**:
- New project wizard with templates
- Visual metadata editor
- Layer manager (add, remove, reorder)
- Content browser for mod files
- Pre-pack validation
- One-click packaging with progress indicator
- Build history

### 5. First-Run Experience

Guided onboarding for new users.

**Steps**:
1. Welcome screen with app introduction
2. League of Legends path detection/selection
3. Brief feature tour
4. Ready to use confirmation

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
2. Enter project name          →    Validate name format
3. Select project location     →    Verify write permissions
4. Configure initial layers    →    Set up layer structure
5. Click "Create"              →    Generate project files
6. [Success]                   →    Open project in editor
```

---

## Data Model

### Settings

| Field | Type | Description |
|-------|------|-------------|
| League Path | Path (optional) | Path to League of Legends installation |
| Mod Storage Path | Path (optional) | Where installed mods are stored |
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

---

## Security Model

### Permissions

The application requests only necessary permissions:

| Permission | Purpose |
|------------|---------|
| File System (Scoped) | Read/write mod files and settings |
| Dialog | File picker and folder selection |
| Shell (Limited) | Open URLs in browser |

### Content Security

- WebView content is loaded from local files only
- No arbitrary code execution from mod files
- Settings and mod index stored in app data directory

### Mod Safety

- Mods are sandboxed within the mod storage directory
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

### Version 1.2

- [ ] Creator Workshop - New project wizard
- [ ] Creator Workshop - Project editor
- [ ] Creator Workshop - Pack to .modpkg

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
| **IPC** | Inter-Process Communication between frontend and backend |
| **Tauri** | Framework for building native desktop apps with web UI |

### Related Documentation

- [league-mod CLI documentation](../league-mod/README.md)
- [ltk_modpkg library](../ltk_modpkg/README.md)
- [ltk_mod_project library](../ltk_mod_project/README.md)
