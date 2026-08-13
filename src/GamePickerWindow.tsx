import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { PgnGameSummary } from "./types";
import "./App.css";

export function GamePickerWindow() {
  const params = new URLSearchParams(window.location.search);
  const path = params.get("path") ?? "";
  const [games, setGames] = useState<PgnGameSummary[]>([]);
  const [error, setError] = useState<string | null>(null);

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
      <div className="modal-table-wrap">
        <table className="explorer-table">
          <thead>
            <tr>
              <th>Blancs</th>
              <th>Noirs</th>
              <th>Date</th>
              <th>Résultat</th>
              <th>Évènement</th>
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
