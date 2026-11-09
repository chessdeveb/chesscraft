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
import { compareText, useClientSort, type Comparators } from "./useClientSort";
import { ColumnResizeHandle, useColumnWidths } from "./useColumnWidths";
import "./App.css";

type GamePickerColumn = "white" | "black" | "date" | "result" | "event";

// Parties d'un fichier PGN, toutes chargees d'un coup : tri cote interface.
const GAME_PICKER_COMPARATORS: Comparators<PgnGameSummary, GamePickerColumn> = {
  white: (a, b) => compareText(a.white, b.white),
  black: (a, b) => compareText(a.black, b.black),
  // "AAAA.MM.JJ" : l'ordre du texte est l'ordre chronologique
  date: (a, b) => compareText(a.date, b.date),
  result: (a, b) => compareText(a.result, b.result),
  event: (a, b) => compareText(a.event, b.event),
};

const GAME_PICKER_HEADERS: { column: GamePickerColumn; labelKey: string }[] = [
  { column: "white", labelKey: "common.white" },
  { column: "black", labelKey: "common.black" },
  { column: "date", labelKey: "common.date" },
  { column: "result", labelKey: "common.result" },
  { column: "event", labelKey: "common.event" },
];

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
  const { sorted, toggleSort, sortArrow } = useClientSort(games, GAME_PICKER_COMPARATORS);

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
              {GAME_PICKER_HEADERS.map(({ column, labelKey }) => (
                <th key={column} className="games-table-sortable" style={{ width: widths[column] }} onClick={() => toggleSort(column)}>
                  {t(labelKey)} {sortArrow(column)}
                  <ColumnResizeHandle onMouseDown={startResize(column)} />
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sorted.map((g) => (
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
