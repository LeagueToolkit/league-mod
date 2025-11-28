# LTK Manager

A desktop application for managing League of Legends mods built with Tauri v2 and React.

## Features

- 📚 **Mod Library** - Install, enable/disable, and manage your mods
- 🔍 **Mod Inspector** - View mod details before installing
- 🛠️ **Creator Tools** - Create and package new mods (coming soon)
- ⚙️ **Settings** - Configure League path and app preferences

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
│   ├── App.tsx            # Main app with routing
│   ├── components/        # Reusable UI components
│   │   ├── Sidebar.tsx
│   │   └── ModCard.tsx
│   ├── pages/             # Page components
│   │   ├── Library.tsx
│   │   └── Settings.tsx
│   ├── lib/               # Utilities and API
│   │   └── tauri.ts
│   └── styles/            # CSS and Tailwind
│       └── app.css
├── public/                # Static assets
├── src-tauri/             # Rust backend (Tauri)
│   ├── Cargo.toml
│   ├── tauri.conf.json    # Tauri configuration
│   ├── capabilities/      # Tauri permissions
│   ├── icons/             # App icons
│   └── src/
│       ├── main.rs        # Tauri entry point
│       ├── commands.rs    # IPC commands
│       ├── state.rs       # App state
│       └── error.rs       # Error types
├── package.json
├── vite.config.ts
└── index.html
```

## Tech Stack

- **Backend**: Rust, Tauri v2
- **Frontend**: React 19, TypeScript, Vite
- **Styling**: Tailwind CSS v4
- **State**: Zustand
- **Icons**: Lucide React

## Tauri Commands

The following IPC commands are available from the frontend:

| Command | Description |
|---------|-------------|
| `get_app_info` | Get app name and version |
| `get_settings` | Get current settings |
| `save_settings` | Save settings |
| `auto_detect_league_path` | Auto-detect League installation |
| `validate_league_path` | Validate a League path |
| `get_installed_mods` | List installed mods |
| `install_mod` | Install a .modpkg file |
| `uninstall_mod` | Uninstall a mod |
| `toggle_mod` | Enable/disable a mod |
| `inspect_modpkg` | Inspect a .modpkg file |
