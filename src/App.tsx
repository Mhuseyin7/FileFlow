import { useState } from "react";
import { Nav, type PageKey } from "./components/Nav";
import { Dashboard } from "./pages/Dashboard";
import { Rules } from "./pages/Rules";
import { Activity } from "./pages/Activity";
import { Review } from "./pages/Review";
import { Folders } from "./pages/Folders";
import { Templates } from "./pages/Templates";
import { Settings } from "./pages/Settings";
import { isStubbed } from "./api";
import { useT } from "./i18n";

export function App() {
  const [page, setPage] = useState<PageKey>("dashboard");
  const { t } = useT();

  return (
    <div className="flex h-full">
      <Nav active={page} onNavigate={setPage} />
      <main className="flex-1 overflow-auto">
        {isStubbed && (
          <div className="border-b border-border bg-warn-soft px-4 py-2 text-xs text-warn">
            {t("app.dev_banner")}
          </div>
        )}
        <div className="mx-auto max-w-6xl px-6 py-6">
          {page === "dashboard" && <Dashboard onNavigate={setPage} />}
          {page === "rules" && <Rules />}
          {page === "activity" && <Activity />}
          {page === "review" && <Review />}
          {page === "folders" && <Folders />}
          {page === "templates" && <Templates />}
          {page === "settings" && <Settings />}
        </div>
      </main>
    </div>
  );
}
