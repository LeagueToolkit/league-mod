import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { LuCircleAlert, LuCircleCheck, LuFolderOpen, LuLoader } from "react-icons/lu";

import { Button, Field, IconButton } from "@/components";
import { api, type Settings } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

export interface SettingsSectionProps {
  settings: Settings;
  onSave: (settings: Settings) => void;
}

export function LeaguePathSection({ settings, onSave }: SettingsSectionProps) {
  const [isDetecting, setIsDetecting] = useState(false);
  const [leaguePathValid, setLeaguePathValid] = useState<boolean | null>(null);

  useEffect(() => {
    if (settings.leaguePath) {
      validatePath(settings.leaguePath);
    } else {
      setLeaguePathValid(null);
    }
  }, [settings.leaguePath]);

  async function validatePath(path: string) {
    try {
      const result = await api.validateLeaguePath(path);
      setLeaguePathValid(unwrapForQuery(result));
    } catch {
      setLeaguePathValid(false);
    }
  }

  async function handleAutoDetect() {
    setIsDetecting(true);
    try {
      const result = await api.autoDetectLeaguePath();
      const path = unwrapForQuery(result);
      if (path) {
        onSave({ ...settings, leaguePath: path, firstRunComplete: true });
      }
    } catch (error) {
      console.error("Failed to auto-detect:", error);
    } finally {
      setIsDetecting(false);
    }
  }

  async function handleBrowse() {
    try {
      const selected = await open({
        directory: true,
        title: "Select League of Legends Installation",
      });

      if (selected) {
        onSave({ ...settings, leaguePath: selected as string, firstRunComplete: true });
      }
    } catch (error) {
      console.error("Failed to browse:", error);
    }
  }

  return (
    <section>
      <h3 className="mb-4 text-lg font-medium text-surface-100">League of Legends</h3>
      <div className="space-y-3">
        <span className="block text-sm font-medium text-surface-400">Installation Path</span>
        <div className="flex gap-2">
          <div className="relative flex-1">
            <Field.Control
              type="text"
              value={settings.leaguePath || ""}
              readOnly
              placeholder="Not configured"
            />
            {settings.leaguePath && (
              <div className="absolute top-1/2 right-3 -translate-y-1/2">
                {leaguePathValid === true && <LuCircleCheck className="h-5 w-5 text-green-500" />}
                {leaguePathValid === false && <LuCircleAlert className="h-5 w-5 text-red-500" />}
              </div>
            )}
          </div>
          <IconButton
            icon={<LuFolderOpen className="h-5 w-5" />}
            variant="outline"
            size="lg"
            onClick={handleBrowse}
          />
        </div>
        <Button
          variant="transparent"
          size="sm"
          onClick={handleAutoDetect}
          loading={isDetecting}
          left={isDetecting ? undefined : <LuLoader className="h-4 w-4" />}
          className="text-brand-400 hover:text-brand-300"
        >
          Auto-detect installation
        </Button>
        {leaguePathValid === false && settings.leaguePath && (
          <p className="text-sm text-red-400">
            Could not find League of Legends at this path. Make sure it points to the folder
            containing the <code className="rounded bg-surface-700 px-1">Game</code> directory.
          </p>
        )}
      </div>
    </section>
  );
}
