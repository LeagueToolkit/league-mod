# Workshop UX Improvements

A comprehensive brainstorm of UX improvements for the Workshop module to make it as robust and professional as possible for thousands of modders.

## 1. Content Editing & Workflow

### Live File Explorer / Content Browser

Embed a file tree viewer inside the project detail page showing the actual `content/{layer}/` directory structure.

- Allow drag-and-drop of game assets (textures, models, etc.) directly into the file tree
- Show file type icons (`.dds`, `.skn`, `.bin`, etc.) and file sizes
- Right-click context menu: Open in Explorer, Delete, Move to Layer, Rename
- This is the single most impactful missing feature — right now there's no way to see or manage the actual mod files from the UI

### Hot Reload / Watch Mode

"Dev Mode" toggle that watches `content/` for file changes and auto-rebuilds the overlay.

- Integrates with the patcher so modders can edit textures in Photoshop, save, and instantly see changes in-game
- Show a small "Rebuilding..." indicator in the status bar
- This is the killer workflow feature for serious modders

### Template System

"New Project from Template" — ship templates for common mod types.

- Templates for: champion skin, map skin, HUD mod, sound mod
- Templates pre-populate the correct WAD structure and directory layout
- Community templates that users can import/share

## 2. Project Management & Organization

### Tags / Categories

Allow users to tag projects for quick filtering.

- Examples: "champion-skin", "map", "hud", "sound", "wip", "released"
- Filter toolbar with tag chips for quick filtering
- Color-coded tags visible on project cards

### Sort Options

Dropdown to sort by multiple criteria.

- Last Modified (current default), Name A-Z, Name Z-A, Date Created, Version
- Persist sort preference in settings

### Bulk Operations

Multi-select mode for batch actions.

- Checkboxes on cards for multi-select
- Bulk pack, bulk delete, bulk export
- "Pack All" button for CI-like workflows where modders want to rebuild everything

### Project Status / Stage Tracking

Visual indicator on cards showing project stage.

- Stages: Draft, In Progress, Ready, Published
- Set manually or auto-inferred from validation state (e.g., "Ready" if no errors/warnings)

### Favorites / Pinning

Pin frequently worked-on projects to the top of the grid.

- Star/pin icon on cards
- Pinned projects always sort first regardless of sort order

## 3. Validation & Quality

### Inline Validation (Always-On)

Show a small health indicator (green/yellow/red dot) on every project card.

- Don't wait for the pack dialog to surface issues — show them proactively
- Click the indicator to see the full validation breakdown
- Run validation in the background on startup and cache results

### Richer Validation Rules

Go beyond basic structural checks.

- Warn on oversized files (textures > 4MB, etc.)
- Warn on common mistakes (wrong directory nesting, mismatched WAD names)
- Suggest fixes: "Did you mean `Aatrox.wad.client` instead of `aatrox.wad.client`?"
- Check for deprecated file formats or patterns
- Validate that referenced champions/maps actually exist in the game

### Pre-Pack Preview

Before packing, show a summary of what will be produced.

- "This .modpkg will contain X files across Y WADs, totaling Z MB"
- Show which game WADs will be affected
- List any conflicts with currently installed mods in the library

## 4. Visual Polish & Feedback

### Thumbnail Improvements

Make thumbnail management more intuitive.

- Drag-and-drop thumbnail upload (not just file picker)
- Crop/resize tool in a modal (simple aspect ratio enforcement for 16:9)
- "Capture from Game" workflow hint — link to a screenshot tool or instructions
- Auto-generate placeholder thumbnails with gradient backgrounds based on project name

### Progress & Activity

Better feedback for all operations.

- Toast notifications for pack success/failure with clickable "Show in Explorer"
- Activity log / history panel: "Packed v1.2.0 at 3:42 PM", "Added layer 'chromas'", etc.
- Undo support for destructive operations (delete project -> 5s undo toast before actual deletion)

### Richer Project Cards

Surface more useful info at a glance.

- Show layer count badge on cards
- Show file count or total size
- Show last pack date (if ever packed)
- Subtle "modified since last pack" indicator

### Animations & Transitions

Polish that makes the app feel professional.

- Skeleton loading states instead of a plain spinner
- Smooth card entrance animations when projects load
- Animated transitions between grid/list views
- Subtle hover lift effect on cards

## 5. Collaboration & Sharing

### Export/Import Project Bundle

Share full editable project workspaces between team members.

- "Export Project" — zip entire project directory (not pack as .modpkg, but the full editable workspace)
- "Import Project" from zip
- Preserves all layers, configs, thumbnails, content

### Changelog / Version History

Track changes across versions.

- Optional changelog textarea per version
- Auto-increment version helper (patch/minor/major buttons next to version field)
- "What changed" diff summary when bumping version

### README Editor

Rich editing for mod descriptions.

- Rich text / markdown editor for the project README
- Preview mode showing rendered markdown
- This becomes the mod description when published

## 6. Strings Editor Improvements

### String Key Autocomplete

Help modders discover the right keys to override.

- Integrate a known game string key database
- As users type keys, suggest matching game strings (e.g., typing "aatrox" suggests `game_character_displayname_Aatrox`)
- Show the current game value next to the key so modders know what they're overriding

### Bulk String Import

Handle large string sets efficiently.

- Import strings from CSV/JSON files
- Copy strings between locales ("Copy en_US to all empty locales")
- "Machine translate" placeholder hint (just flag it, don't actually translate)

### String Preview

Contextualize string overrides.

- Show a formatted preview of what the string will look like in-game context
- Highlight which strings have been overridden vs. default

## 7. Layer Management Improvements

### Layer Content Visualization

Show what each layer contains.

- Show file count and total size per layer
- Expandable file list under each layer card
- Visual indicator of which layers override the same files (conflict preview)

### Layer Diff View

Understand interactions between layers.

- Select two layers and see which files overlap
- Helps modders understand layer interactions before packing

### Quick Layer Duplication

Speed up creation of layer variations.

- "Duplicate Layer" to quickly create variations (e.g., chromas)
- Copies all content and string overrides

## 8. Onboarding & Discoverability

### First-Run Guided Setup

Help new modders get started quickly.

- When workshop path is first configured, offer a walkthrough
- "Create your first mod" wizard with step-by-step guidance
- Contextual tooltips on first visit to each tab

### Contextual Help

Reduce confusion around complex concepts.

- Small `?` icons next to complex fields (layers, string overrides, slug)
- Links to documentation or video tutorials
- "What is a layer?" / "What are string overrides?" explainers

### Keyboard Shortcuts

Power-user efficiency.

| Shortcut | Action |
|----------|--------|
| `Ctrl+N` | New Project |
| `Ctrl+P` | Pack current project |
| `Ctrl+F` | Focus search |
| `Ctrl+S` | Save current form |
| `Esc` | Back to project list |

Show shortcut hints in button tooltips.

## 9. Performance & Scale

### Pagination or Virtualization

Handle large project collections gracefully.

- When users have 50+ projects, virtualize the grid (TanStack Virtual)
- Lazy-load thumbnails as they scroll into view

### Background Validation

Make health indicators instant.

- Run validation for all projects in the background on startup
- Cache results so card health indicators appear without delay

### Search Improvements

More powerful project discovery.

- Fuzzy search (typo-tolerant)
- Search across description and author fields, not just name/slug
- Recent searches / search history

## 10. Platform Integration

### "Open in Editor" Button

Bridge the gap between the app and modders' tools.

- Open the project's content directory in VS Code, or the system file explorer
- Configurable editor path in settings

### Git Integration (Future)

Version control for serious modders.

- Detect if project is a git repo
- Show git status on project card
- Commit/push from within the app
- Version history via git log

---

## Priority Recommendations

The five highest-impact improvements to tackle first:

| Priority | Feature | Why |
|----------|---------|-----|
| 1 | **Content File Browser** | Without this, modders constantly switch between the app and file explorer. Table stakes for a professional modding tool. |
| 2 | **Always-On Validation Indicators** | Proactive quality feedback on cards eliminates the "pack and pray" workflow. |
| 3 | **Skeleton Loading + Animations** | Cheapest way to make the whole app feel significantly more polished. |
| 4 | **Sort / Filter / Tags** | Essential once a modder has more than ~10 projects. |
| 5 | **Hot Reload Dev Mode** | The feature that would make modders *choose* this tool over alternatives. |
