import type { AppInfo } from "@/lib/tauri";

interface AboutSectionProps {
  appInfo: AppInfo | undefined;
}

export function AboutSection({ appInfo }: AboutSectionProps) {
  return (
    <section>
      <h3 className="mb-4 text-lg font-medium text-surface-100">About</h3>
      <div className="rounded-lg border border-surface-600 bg-surface-800 p-4">
        <div className="flex items-center justify-between">
          <div>
            <h4 className="font-medium text-surface-100">LTK Manager</h4>
            {appInfo && <p className="text-sm text-surface-500">Version {appInfo.version}</p>}
          </div>
        </div>
        <p className="mt-3 text-sm text-surface-400">
          LTK Manager is part of the LeagueToolkit project. It provides a graphical interface for
          managing League of Legends mods using the modpkg format.
        </p>
        <div className="mt-4 flex gap-4 border-t border-surface-600 pt-4">
          <a
            href="https://github.com/LeagueToolkit/league-mod"
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm text-brand-400 transition-colors hover:text-brand-300"
          >
            View on GitHub →
          </a>
          <a
            href="https://github.com/LeagueToolkit/league-mod/wiki"
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm text-brand-400 transition-colors hover:text-brand-300"
          >
            Documentation →
          </a>
        </div>
      </div>
    </section>
  );
}
