import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { HomeDockview } from "./HomeDockview";
import { ImportBasePicker } from "./components/ImportBasePicker";
import i18n, { useSyncLanguage } from "./i18n";
import type { BulkImportSummary, PgnGameSummary, TabDto } from "./types";
import "./App.css";

type BulkImportResult = { kind: "done"; summary: BulkImportSummary } | { kind: "error"; message: string };

function formatBulkImportSummary(summary: BulkImportSummary): string {
  return i18n.t("homeWindow.importSummary", {
    total: summary.total,
    imported: summary.imported,
    duplicates: summary.skippedDuplicates,
    invalid: summary.skippedInvalid,
    failed: summary.failed,
  });
}

// Creer la fenetre Partie depuis une commande Rust (WebviewWindowBuilder
// invoque via invoke_handler) plante systematiquement sous Windows, avec ou
// sans menu attache -- confirme a plusieurs reprises. On la cree donc cote
// JS, seule approche stable trouvee.
async function openTabInGameWindow(tab: TabDto): Promise<void> {
  const existing = await WebviewWindow.getByLabel("game");
  if (existing) {
    await existing.setFocus();
    await emit("game-tab-opened", tab);
    return;
  }
  const win = new WebviewWindow("game", {
    url: `/?window=game&tabId=${tab.tabId}`,
    title: i18n.t("homeWindow.gameWindowTitle"),
    width: 1100,
    height: 750,
    dragDropEnabled: false,
  });
  win.once("tauri://error", (e) => console.error(e.payload));
}

export function HomeWindow() {
  useSyncLanguage();
  const { t } = useTranslation();
  const [homeFocusPath, setHomeFocusPath] = useState<string | null>(null);
  const [basesVersion, setBasesVersion] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const [bulkImportPgnPath, setBulkImportPgnPath] = useState<string | null>(null);
  const [bulkImportProgress, setBulkImportProgress] = useState<BulkImportSummary | null>(null);
  const [bulkImportResult, setBulkImportResult] = useState<BulkImportResult | null>(null);

  const handleFocusConsumed = useCallback(() => setHomeFocusPath(null), []);

  const importGameAt = useCallback(async (path: string, index: number) => {
    try {
      const tab = await invoke<TabDto>("import_pgn", { path, index });
      await openTabInGameWindow(tab);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleOpenGame = useCallback(async (path: string, id: number) => {
    try {
      const tab = await invoke<TabDto>("open_game", { path, id });
      await openTabInGameWindow(tab);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleNewGame = useCallback(async () => {
    try {
      const tab = await invoke<TabDto>("new_game");
      await openTabInGameWindow(tab);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleImport = useCallback(async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "PGN", extensions: ["pgn"] }],
      });
      if (!path) return;
      const games = await invoke<PgnGameSummary[]>("list_pgn_games", { path });
      if (games.length <= 1) {
        await importGameAt(path, 0);
        return;
      }
      const picker = new WebviewWindow("game-picker", {
        url: `/?window=game-picker&path=${encodeURIComponent(path)}`,
        title: t("homeWindow.pickerWindowTitle"),
        width: 720,
        height: 480,
      });
      picker.once("tauri://error", (e) => setError(String(e.payload)));
    } catch (e) {
      setError(String(e));
    }
  }, [importGameAt, t]);

  const handleBulkImportClick = useCallback(async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "PGN", extensions: ["pgn"] }],
      });
      if (!path) return;
      setBulkImportPgnPath(path);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleBulkImportConfirm = useCallback(
    (basePath: string) => {
      // Ne jamais mettre d'effet de bord (invoke) dans le callback d'un
      // setState : en StrictMode (dev), React l'appelle deux fois pour
      // detecter les impuretes, ce qui declenchait litteralement deux
      // imports concurrents sur le meme fichier -- source des millions
      // d'"echecs" observes (verrous SQLite entre les deux ecrivains).
      if (!bulkImportPgnPath) return;
      const pgnPath = bulkImportPgnPath;
      setBulkImportPgnPath(null);
      setBulkImportProgress({ total: 0, imported: 0, skippedDuplicates: 0, skippedInvalid: 0, failed: 0 });
      invoke("bulk_import_pgn", { dbPath: basePath, pgnPath }).catch((e) => {
        setBulkImportProgress(null);
        setBulkImportResult({ kind: "error", message: String(e) });
      });
    },
    [bulkImportPgnPath]
  );

  useEffect(() => {
    const unlistenPromises = [
      listen<BulkImportSummary>("bulk-import-progress", (event) => setBulkImportProgress(event.payload)),
      listen<BulkImportSummary>("bulk-import-done", (event) => {
        setBulkImportProgress(null);
        setBulkImportResult({ kind: "done", summary: event.payload });
        setBasesVersion((v) => v + 1);
      }),
      listen<string>("bulk-import-error", (event) => {
        setBulkImportProgress(null);
        setBulkImportResult({ kind: "error", message: event.payload });
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, []);

  // selection faite dans la fenetre separee de choix de partie
  useEffect(() => {
    const unlistenPromise = listen<{ path: string; index: number }>("import-game-selected", (event) => {
      importGameAt(event.payload.path, event.payload.index);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [importGameAt]);

  // selection faite dans la fenetre separee "emplacement des bases"
  useEffect(() => {
    const unlistenPromise = listen<string>("select-base", (event) => {
      setHomeFocusPath(event.payload);
      getCurrentWindow().setFocus();
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // la fenetre Partie previent l'accueil qu'une base a change (sauvegarde, suppression...)
  useEffect(() => {
    const unlistenPromise = listen("bases-changed", () => {
      setBasesVersion((v) => v + 1);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // --- Menus natifs ---
  useEffect(() => {
    const unlistenPromises = [
      listen("menu-new-game", () => handleNewGame()),
      listen("menu-import-pgn", () => handleImport()),
      listen("menu-bulk-import-pgn", () => handleBulkImportClick()),
      listen("menu-show-bases-paths", () => {
        const win = new WebviewWindow("bases-paths", {
          url: "/?window=bases-paths",
          title: t("homeWindow.basesPathsWindowTitle"),
          width: 640,
          height: 420,
        });
        win.once("tauri://error", (e) => setError(String(e.payload)));
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [handleNewGame, handleImport, handleBulkImportClick, t]);

  return (
    <div className="app-layout">
      {error && <p className="error">{error}</p>}
      <HomeDockview
        focusBasePath={homeFocusPath}
        onOpenGame={handleOpenGame}
        onFocusConsumed={handleFocusConsumed}
        onOpenPgnGame={importGameAt}
        onImportNewFile={handleImport}
        basesVersion={basesVersion}
      />
      {bulkImportPgnPath && (
        <ImportBasePicker
          pgnPath={bulkImportPgnPath}
          onConfirm={handleBulkImportConfirm}
          onCancel={() => setBulkImportPgnPath(null)}
        />
      )}
      {bulkImportProgress && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>{t("homeWindow.importing")}</h2>
            <p>{formatBulkImportSummary(bulkImportProgress)}</p>
            <div className="modal-actions">
              <button onClick={() => invoke("cancel_bulk_import")}>{t("common.cancel")}</button>
            </div>
          </div>
        </div>
      )}
      {bulkImportResult && (
        <div className="modal-overlay">
          <div className="modal">
            {bulkImportResult.kind === "done" ? (
              <>
                <h2>{t("homeWindow.importDone")}</h2>
                <p>{formatBulkImportSummary(bulkImportResult.summary)}</p>
              </>
            ) : (
              <>
                <h2>{t("homeWindow.importFailed")}</h2>
                <p className="error">{bulkImportResult.message}</p>
              </>
            )}
            <div className="modal-actions">
              <button
                onClick={() =>
                  navigator.clipboard.writeText(
                    bulkImportResult.kind === "done"
                      ? formatBulkImportSummary(bulkImportResult.summary)
                      : bulkImportResult.message,
                  )
                }
              >
                {t("common.copy")}
              </button>
              <button onClick={() => setBulkImportResult(null)}>{t("common.ok")}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
