import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { Library } from "./pages/Library";
import { Settings } from "./pages/Settings";

type Page = "library" | "creator" | "settings";

interface AppInfo {
  name: string;
  version: string;
}

function App() {
  const [currentPage, setCurrentPage] = useState<Page>("library");
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    invoke<AppInfo>("get_app_info").then(setAppInfo);
  }, []);

  return (
    <div className="flex h-screen bg-surface-950">
      <Sidebar
        currentPage={currentPage}
        onNavigate={setCurrentPage}
        appVersion={appInfo?.version}
      />
      <main className="flex-1 overflow-hidden">
        {currentPage === "library" && <Library />}
        {currentPage === "settings" && <Settings />}
      </main>
    </div>
  );
}

export default App;
