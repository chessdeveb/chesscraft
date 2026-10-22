import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { useSyncLanguage } from "./i18n";
import { useSyncBoardTheme } from "./boardTheme";
import { useSyncColorScheme } from "./colorScheme";
import { useAutoDismissError } from "./useAutoDismissError";
import type { PgnGameSummary } from "./types";
import { ColumnResizeHandle, useColumnWidths } from "./useColumnWidths";
import "./App.css";

const GAME_PICKER_COLUMNS = [
  { key: "white", default: 140 },
  { key: "black", default: 140 },
  { key: "date", default: 90 },
  { key: "result", default: 70 },
  { key: "event", default: 180 },
];

export function GamePickerWindow() {
  useSyncLanguage();
  useSyncBoardTheme();
  useSyncColorScheme();
  const { t } = useTranslation();
  const params = new URLSearchParams(window.location.search);
  const path = params.get("path") ?? "";
  const [games, setGames] = useState<PgnGameSummary[]>([]);
  const [error, setError] = useAutoDismissError();
  const { widths, startResize, totalWidth } = useColumnWidths("gamePicker", GAME_PICKER_COLUMNS);

  // Petite fenetre utilitaire : pas de menu natif souhaite (voir
  // lib.rs::remove_window_menu -- sous Windows, une fenetre creee sans menu
  // explicite peut malgre tout heriter du menu partage de l'appli, de facon
  // intermittente).
  useEffect(() => {
    invoke("remove_window_menu").catch(() => {});
  }, []);

  useEffect(() => {
    invoke<PgnGameSummary[]>("list_pgn_games", { path })
      .then(setGames)
      .catch((e) => setError(String(e)));
  }, [path]);

  async function handleSelect(index: number) {
    await emit("import-game-selected", { path, index });
    await getCurrentWindow().close();
  }

  return (
    <main className="picker-window">
      <p className="picker-path">{path}</p>
      {error && <p className="error">{error}</p>}
      <div className="modal-table-wrap resizable-table-wrap">
        <table className="explorer-table resizable-table" style={{ width: totalWidth }}>
          <thead>
            <tr>
              <th style={{ width: widths.white }}>
                {t("common.white")}
                <ColumnResizeHandle onMouseDown={startResize("white")} />
              </th>
              <th style={{ width: widths.black }}>
                {t("common.black")}
                <ColumnResizeHandle onMouseDown={startResize("black")} />
              </th>
              <th style={{ width: widths.date }}>
                {t("common.date")}
                <ColumnResizeHandle onMouseDown={startResize("date")} />
              </th>
              <th style={{ width: widths.result }}>
                {t("common.result")}
                <ColumnResizeHandle onMouseDown={startResize("result")} />
              </th>
              <th style={{ width: widths.event }}>
                {t("common.event")}
                <ColumnResizeHandle onMouseDown={startResize("event")} />
              </th>
            </tr>
          </thead>
          <tbody>
            {games.map((g) => (
              <tr key={g.index} onClick={() => handleSelect(g.index)}>
                <td>{g.white}</td>
                <td>{g.black}</td>
                <td>{g.date}</td>
                <td>{g.result}</td>
                <td>{g.event}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </main>
  );
}
