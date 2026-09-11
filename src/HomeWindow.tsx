import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ask, message, open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { HomeDockview } from "./HomeDockview";
import { ImportBasePicker } from "./components/ImportBasePicker";
import { ToolBasePicker } from "./components/ToolBasePicker";
import i18n, { useSyncLanguage } from "./i18n";
import { useSyncBoardTheme } from "./boardTheme";
import { useSyncColorScheme } from "./colorScheme";
import { useAutoDismissError } from "./useAutoDismissError";
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

// Correspond a lib.rs::ToolRebuildProgress (RebuildProgress applati avec
// "stage") : rebuild_prefix_stats/rebuild_player_stats ont chacune une phase
// de lecture (parties examinees, bornee par la taille de la base) suivie
// d'une phase d'ecriture (lignes distinctes inserees dans la table
// precalculee, dont le total depasse souvent le nombre de parties depuis que
// PRECOMPUTED_MAX_DEPTH est passe a 20) -- les additionner en un seul nombre
// continu (essaye d'abord) donnait un total sans rapport clair avec la
// taille de la base, rapporte par l'utilisateur comme deroutant.
type ToolRebuildProgressEvent = { stage: "openings" | "players" } & (
  | { phase: "reading"; examined: number }
  | { phase: "writing"; written: number; total: number }
);

function formatRebuildProgress(event: ToolRebuildProgressEvent): string {
  const stage = i18n.t(event.stage === "openings" ? "tools.stageOpenings" : "tools.stagePlayers");
  return event.phase === "reading"
    ? i18n.t("tools.progressRebuildReading", { stage, examined: event.examined })
    : i18n.t("tools.progressRebuildWriting", { stage, written: event.written, total: event.total });
}

// Fenetre a instance unique (about, position-editor, game-picker...) deja
// ouverte : Tauri refuse de creer une deuxieme fenetre avec la meme etiquette
// ("a webview with label `x` already exists") -- rapporte par l'utilisateur
// comme un message d'erreur (avec un flou d'ecran, probablement un effet de
// la tentative de creation qui echoue) en rouvrant Aide deux fois. On
// ramene juste la fenetre existante au premier plan a la place.
async function focusIfAlreadyOpen(label: string): Promise<boolean> {
  const existing = await WebviewWindow.getByLabel(label);
  if (!existing) return false;
  await existing.setFocus();
  return true;
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
    url: `/?window=game&tabId=${tab.tabId}${tab.isPlayerPrep ? "&prep=1" : ""}`,
    title: i18n.t("homeWindow.gameWindowTitle"),
    width: 1100,
    height: 750,
    dragDropEnabled: false,
  });
  win.once("tauri://error", (e) => console.error(e.payload));
}

export function HomeWindow() {
  useSyncLanguage();
  useSyncBoardTheme();
  useSyncColorScheme();
  const { t } = useTranslation();
  const [basesVersion, setBasesVersion] = useState(0);
  const [error, setError] = useAutoDismissError();

  const [bulkImportPgnPath, setBulkImportPgnPath] = useState<string | null>(null);
  // Operation de maintenance choisie via le menu Outils : reste posee pour
  // toute la duree du flux (choix de la base, execution, et pour Dedoublonner
  // le compte intermediaire avant confirmation), remise a null seulement une
  // fois le flux termine ou annule.
  const [toolAction, setToolAction] = useState<"rebuild-stats" | "deduplicate" | "backfill-fide" | null>(null);
  // Affichage du choix de base : separe de toolAction, qui doit rester pose
  // pendant l'execution (voir toolJobRunning) sans que ToolBasePicker se
  // rouvre entre-temps.
  const [toolPickerOpen, setToolPickerOpen] = useState(false);
  // true pendant l'execution effective (apres le choix de la base), le temps
  // de recevoir l'evenement -done/-error correspondant (voir plus bas) -- ces
  // commandes tournent sur un thread dedie cote Rust (voir lib.rs) : une
  // premiere version synchrone (calcul fait directement dans le handler de la
  // commande) bloquait la boucle de messages Win32 assez longtemps, sur la
  // base de reference reelle de l'utilisateur (plusieurs millions de
  // parties), pour que Windows signale carrement l'appli comme "ne repond
  // plus" au moindre clic -- meme avec ce message affiche, donc pas
  // seulement un probleme de retour visuel manquant.
  const [toolJobRunning, setToolJobRunning] = useState(false);
  // Message deja mis en forme, affiche pendant l'execution (voir les
  // evenements "tool-*-progress" plus bas) : chaque operation a sa propre
  // forme de progression (Recalculer les stats distingue lecture/ecriture,
  // Dedoublonner/Resoudre les identifiants FIDE comptent examinees+trouvees) --
  // les formater au moment de recevoir l'evenement, plutot que de stocker
  // les nombres bruts et choisir un format a l'affichage, evite de melanger
  // des formes differentes dans un seul etat. Remis a null a chaque nouvelle
  // etape (voir setToolProgress(null) avant chaque invoke).
  const [toolProgress, setToolProgress] = useState<string | null>(null);
  // Base choisie pour l'operation en cours : conserve hors etat React (pas
  // besoin de re-rendu) pour que le listener de "tool-count-duplicates-done"
  // (enregistre une seule fois, voir plus bas) puisse relancer
  // deduplicate_base sur la meme base apres confirmation.
  const toolBasePathRef = useRef<string | null>(null);
  const [bulkImportProgress, setBulkImportProgress] = useState<BulkImportSummary | null>(null);
  const [bulkImportResult, setBulkImportResult] = useState<BulkImportResult | null>(null);
  // sans indicateur, lire un fichier PGN pas encore synchronise localement
  // (OneDrive/Dropbox "en ligne uniquement") peut prendre plusieurs secondes
  // sans aucun retour visuel -- donne l'impression que l'appli a plante.
  const [pgnLoading, setPgnLoading] = useState(false);

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

  const handlePrepareAgainstPlayer = useCallback(async (name: string, white: boolean) => {
    try {
      const tab = await invoke<TabDto>("open_player_prep_tab", { name, white });
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

  const handleNewPosition = useCallback(async () => {
    if (await focusIfAlreadyOpen("position-editor")) return;
    const editor = new WebviewWindow("position-editor", {
      url: "/?window=position-editor",
      title: t("homeWindow.positionEditorWindowTitle"),
      width: 620,
      height: 560,
      resizable: false,
    });
    editor.once("tauri://error", (e) => setError(String(e.payload)));
  }, [t]);

  const handlePositionEditorConfirmed = useCallback(async (fen: string) => {
    try {
      const tab = await invoke<TabDto>("new_game_from_fen", { fen });
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
      setPgnLoading(true);
      const games = await invoke<PgnGameSummary[]>("list_pgn_games", { path });
      if (games.length <= 1) {
        await importGameAt(path, 0);
        return;
      }
      if (await focusIfAlreadyOpen("game-picker")) return;
      const picker = new WebviewWindow("game-picker", {
        url: `/?window=game-picker&path=${encodeURIComponent(path)}`,
        title: t("homeWindow.pickerWindowTitle"),
        width: 720,
        height: 480,
      });
      picker.once("tauri://error", (e) => setError(String(e.payload)));
    } catch (e) {
      setError(String(e));
    } finally {
      setPgnLoading(false);
    }
  }, [importGameAt, t]);

  const handleShowAbout = useCallback(async () => {
    if (await focusIfAlreadyOpen("about")) return;
    const win = new WebviewWindow("about", {
      url: "/?window=about",
      title: t("homeWindow.aboutWindowTitle"),
      width: 420,
      height: 320,
      resizable: false,
    });
    win.once("tauri://error", (e) => setError(String(e.payload)));
  }, [t]);

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

  // Titre affiche par ToolBasePicker : un seul modal pour les trois
  // operations de maintenance, distinguees par toolAction.
  const toolActionTitle =
    toolAction === "rebuild-stats"
      ? t("menu.tools.rebuildExplorerStats")
      : toolAction === "deduplicate"
        ? t("menu.tools.deduplicateBase")
        : t("menu.tools.backfillFideIds");

  // Ne fait que declencher la commande correspondante : le resultat arrive
  // de facon asynchrone via un evenement "tool-*-done"/"tool-*-error" (voir
  // l'effet juste apres), pas via la promesse renvoyee par invoke() (qui ne
  // signale que le demarrage du thread cote Rust, voir begin_tool_job).
  const handleToolBaseChosen = useCallback(
    (path: string) => {
      const action = toolAction;
      setToolPickerOpen(false);
      if (!action) return;
      toolBasePathRef.current = path;
      setToolJobRunning(true);
      setToolProgress(null);
      const command =
        action === "rebuild-stats" ? "rebuild_explorer_stats" : action === "deduplicate" ? "count_duplicate_games" : "backfill_fide_ids";
      invoke(command, { path }).catch((e) => {
        setToolJobRunning(false);
        setToolAction(null);
        setError(String(e));
      });
    },
    [toolAction]
  );

  // Resultats des operations de maintenance du menu Outils, emis depuis un
  // thread dedie cote Rust (voir lib.rs) une fois le travail termine.
  // Dedoublonner est en deux etapes : le compte (tool-count-duplicates-done)
  // declenche une confirmation puis, si accepte, relance deduplicate_base sur
  // la meme base (toolBasePathRef) -- toolAction reste pose jusqu'a la toute
  // fin du flux pour que le titre affiche par le modal "en cours" (voir plus
  // bas) reste correct pendant cette deuxieme etape.
  useEffect(() => {
    const unlistenPromises = [
      listen<ToolRebuildProgressEvent>("tool-rebuild-stats-progress", (event) => setToolProgress(formatRebuildProgress(event.payload))),
      // etape 2/2 de Dedoublonner (rebuild des stats une fois des doublons
      // supprimes) : evenement distinct de tool-deduplicate-progress
      // (etape 1, forme differente -- examinees/trouvees), voir lib.rs.
      listen<ToolRebuildProgressEvent>("tool-deduplicate-rebuild-progress", (event) => setToolProgress(formatRebuildProgress(event.payload))),
      listen<{ examined: number; matched: number }>("tool-count-duplicates-progress", (event) =>
        setToolProgress(t("tools.progressDuplicates", event.payload))
      ),
      listen<{ examined: number; matched: number }>("tool-deduplicate-progress", (event) =>
        setToolProgress(t("tools.progressDuplicates", event.payload))
      ),
      listen<{ examined: number; matched: number }>("tool-backfill-fide-ids-progress", (event) =>
        setToolProgress(t("tools.progressResolved", event.payload))
      ),
      listen("tool-rebuild-stats-done", async () => {
        setToolJobRunning(false);
        setToolAction(null);
        await message(t("tools.rebuildStatsDone"), { title: t("menu.tools.rebuildExplorerStats") });
      }),
      listen<string>("tool-rebuild-stats-error", (event) => {
        setToolJobRunning(false);
        setToolAction(null);
        setError(event.payload);
      }),
      listen<number>("tool-count-duplicates-done", async (event) => {
        setToolJobRunning(false);
        const count = event.payload;
        if (count === 0) {
          setToolAction(null);
          await message(t("tools.noDuplicatesFound"), { title: t("menu.tools.deduplicateBase") });
          return;
        }
        const confirmed = await ask(t("tools.deduplicateConfirm", { count }), {
          title: t("menu.tools.deduplicateBase"),
          kind: "warning",
        });
        if (!confirmed || !toolBasePathRef.current) {
          setToolAction(null);
          return;
        }
        setToolJobRunning(true);
        setToolProgress(null);
        invoke("deduplicate_base", { path: toolBasePathRef.current }).catch((e) => {
          setToolJobRunning(false);
          setToolAction(null);
          setError(String(e));
        });
      }),
      listen<string>("tool-count-duplicates-error", (event) => {
        setToolJobRunning(false);
        setToolAction(null);
        setError(event.payload);
      }),
      listen<number>("tool-deduplicate-done", async (event) => {
        setToolJobRunning(false);
        setToolAction(null);
        await message(t("tools.deduplicateDone", { count: event.payload }), { title: t("menu.tools.deduplicateBase") });
        setBasesVersion((v) => v + 1);
      }),
      listen<string>("tool-deduplicate-error", (event) => {
        setToolJobRunning(false);
        setToolAction(null);
        setError(event.payload);
      }),
      listen<number>("tool-backfill-fide-ids-done", async (event) => {
        setToolJobRunning(false);
        setToolAction(null);
        await message(t("tools.backfillFideIdsDone", { count: event.payload }), { title: t("menu.tools.backfillFideIds") });
      }),
      listen<string>("tool-backfill-fide-ids-error", (event) => {
        setToolJobRunning(false);
        setToolAction(null);
        setError(event.payload);
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [t]);

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

  // la fenetre Partie previent l'accueil qu'une base a change (sauvegarde, suppression...)
  useEffect(() => {
    const unlistenPromise = listen("bases-changed", () => {
      setBasesVersion((v) => v + 1);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // la fenetre Nouvelle position previent l'accueil du FEN choisi une fois validee
  useEffect(() => {
    const unlistenPromise = listen<string>("position-editor-confirmed", (event) => {
      handlePositionEditorConfirmed(event.payload);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [handlePositionEditorConfirmed]);

  // --- Menus natifs ---
  useEffect(() => {
    const unlistenPromises = [
      listen("menu-new-game", () => handleNewGame()),
      listen("menu-new-position", () => handleNewPosition()),
      listen("menu-import-pgn", () => handleImport()),
      listen("menu-bulk-import-pgn", () => handleBulkImportClick()),
      listen("menu-rebuild-explorer-stats", () => {
        setToolAction("rebuild-stats");
        setToolPickerOpen(true);
      }),
      listen("menu-deduplicate-base", () => {
        setToolAction("deduplicate");
        setToolPickerOpen(true);
      }),
      listen("menu-backfill-fide-ids", () => {
        setToolAction("backfill-fide");
        setToolPickerOpen(true);
      }),
      listen("menu-show-about", () => handleShowAbout()),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [handleNewGame, handleNewPosition, handleImport, handleBulkImportClick, handleShowAbout]);

  // --- Raccourcis clavier ---
  // Les accelerateurs natifs du menu (voir build_menu, Rust) ne se
  // declenchent jamais au clavier sous Windows : muda (la lib de menu
  // utilisee par Tauri) documente que ca necessite un relais TranslateAcceleratorW
  // dans la boucle de messages Win32, que Tauri ne fournit pas -- seul le clic
  // sur l'item de menu fonctionne (voir les listeners menu-* ci-dessus). On
  // implemente donc les raccourcis nous-memes au niveau JS ; l'accelerateur
  // Rust reste uniquement pour son affichage ("Ctrl+N" a cote du libelle).
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "F1") {
        e.preventDefault();
        handleShowAbout();
        return;
      }
      if (!(e.ctrlKey || e.metaKey)) return;
      const key = e.key.toLowerCase();
      if (key === "n") {
        e.preventDefault();
        handleNewGame();
      } else if (key === "p") {
        e.preventDefault();
        handleNewPosition();
      } else if (key === "o") {
        e.preventDefault();
        handleImport();
      } else if (key === "i") {
        e.preventDefault();
        handleBulkImportClick();
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleNewGame, handleNewPosition, handleImport, handleBulkImportClick, handleShowAbout]);

  return (
    <div className="app-layout">
      {error && <p className="error">{error}</p>}
      {pgnLoading && <p className="status-message">{t("common.loading")}</p>}
      <HomeDockview
        onOpenGame={handleOpenGame}
        onOpenPgnGame={importGameAt}
        onImportNewFile={handleImport}
        basesVersion={basesVersion}
        onPrepareAgainstPlayer={handlePrepareAgainstPlayer}
      />
      {bulkImportPgnPath && (
        <ImportBasePicker
          pgnPath={bulkImportPgnPath}
          onConfirm={handleBulkImportConfirm}
          onCancel={() => setBulkImportPgnPath(null)}
        />
      )}
      {toolPickerOpen && (
        <ToolBasePicker
          title={toolActionTitle}
          onConfirm={handleToolBaseChosen}
          onCancel={() => {
            setToolPickerOpen(false);
            setToolAction(null);
          }}
        />
      )}
      {toolJobRunning && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>{toolActionTitle}</h2>
            <p className="status-message">{toolProgress ?? t("tools.working")}</p>
          </div>
        </div>
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
