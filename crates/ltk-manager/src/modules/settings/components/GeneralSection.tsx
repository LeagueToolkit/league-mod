import type { Settings } from "@/lib/tauri";

import { LeaguePathSection } from "./LeaguePathSection";
import { ModStorageSection } from "./ModStorageSection";
import { WorkshopSection } from "./WorkshopSection";

interface GeneralSectionProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function GeneralSection({ settings, onSave }: GeneralSectionProps) {
  return (
    <div className="space-y-8">
      <LeaguePathSection settings={settings} onSave={onSave} />
      <ModStorageSection settings={settings} onSave={onSave} />
      <WorkshopSection settings={settings} onSave={onSave} />
    </div>
  );
}
