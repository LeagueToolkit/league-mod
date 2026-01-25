# LTK Manager

A desktop application for managing League of Legends mods built with Tauri v2 and React.

## Features

- 📚 **Mod Library** - Install, enable/disable, and manage your mods
  - Supports `.modpkg` and `.fantome` file formats
  - Thumbnail display for mods with images
  - Click-to-toggle mod enable/disable
  - Drag and drop installation
- 🔍 **Mod Inspector** - View mod details before installing
- 🎨 **Theming** - VS Code-inspired dark/light themes with accent color customization
- 🛠️ **Creator Tools** - Create and package new mods (coming soon)
- ⚙️ **Settings** - Configure League path, theme, and app preferences

## Development

### Prerequisites

- [Rust](https://rustup.rs/) (1.70+)
- [Node.js](https://nodejs.org/) (20+)
- [pnpm](https://pnpm.io/)

### Setup

```bash
# Navigate to the manager directory
cd crates/ltk-manager

# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev
```

### Building

```bash
# Build for production
pnpm tauri build
```

The built application will be in `src-tauri/target/release/bundle/`.

## Project Structure

```
ltk-manager/
├── src/                    # React frontend
│   ├── main.tsx           # React entry point
│   ├── components/        # Reusable UI components
│   │   ├── Sidebar.tsx    # Navigation sidebar
│   │   ├── ModCard.tsx    # Mod display card with thumbnail
│   │   └── Button.tsx     # Button components
│   ├── pages/             # Page components
│   │   ├── Library.tsx    # Mod library page
│   │   └── Settings.tsx   # Settings page with theming
│   ├── modules/           # Feature modules
│   │   ├── library/       # Mod library API
│   │   ├── settings/      # Settings and theming
│   │   └── patcher/       # Mod patcher integration
│   ├── lib/               # Utilities and API
│   │   └── tauri.ts       # Tauri type definitions
│   └── styles/            # CSS with Tailwind v4
│       └── app.css        # Theme variables and styles
├── public/                # Static assets
├── src-tauri/             # Rust backend (Tauri)
│   ├── Cargo.toml
│   ├── tauri.conf.json    # Tauri configuration
│   ├── capabilities/      # Tauri permissions
│   └── src/
│       ├── main.rs        # Tauri entry point
│       ├── commands/      # IPC command handlers
│       ├── mods/          # Mod management logic
│       ├── patcher/       # Overlay patcher
│       └── state.rs       # App state and settings
├── package.json
├── vite.config.ts
└── index.html
```

## Tech Stack

- **Backend**: Rust, Tauri v2
- **Frontend**: React 19, TypeScript, Vite
- **Styling**: Tailwind CSS v4
- **State**: TanStack Query, Zustand

## Tauri Commands

The following IPC commands are available from the frontend:

| Command | Description |
|---------|-------------|
| `get_app_info` | Get app name and version |
| `get_settings` | Get current settings |
| `save_settings` | Save settings (theme, accent, paths) |
| `auto_detect_league_path` | Auto-detect League installation |
| `validate_league_path` | Validate a League path |
| `check_setup_required` | Check if first-run setup is needed |
| `get_installed_mods` | List installed mods with thumbnails |
| `install_mod` | Install a .modpkg or .fantome file |
| `uninstall_mod` | Uninstall a mod |
| `toggle_mod` | Enable/disable a mod |
| `inspect_modpkg` | Inspect a .modpkg file |
| `get_mod_thumbnail` | Get mod thumbnail as base64 data URL |
| `reveal_in_explorer` | Open file location in explorer |
| `start_patcher` | Start the overlay patcher |
| `stop_patcher` | Stop the overlay patcher |
| `get_patcher_status` | Get patcher running status |

## Supported Mod Formats

- **`.modpkg`** - LeagueToolkit mod package format
- **`.fantome`** - Legacy Fantome mod format (auto-converted)

Both formats support:
- Mod metadata (name, version, author, description)
- Thumbnail images (displayed in mod library)
- Multiple layers/variants

