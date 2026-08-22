import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { BasesTable } from "./BasesTable";
import type { BaseInfo } from "../types";

interface SaveBasePickerProps {
  activeBasePath: string | null;
  tags: Record<string, string>;
  onConfirm: (path: string) => void;
  onCancel: () => void;
}

function baseNameFromPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? path;
  return fileName.replace(/\.sqlite$/i, "");
}

const RESULT_OPTIONS = ["*", "1-0", "0-1", "1/2-1/2"];

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

function todayPgnDate(): string {
  const d = new Date();
  return `${d.getFullYear()}.${pad2(d.getMonth() + 1)}.${pad2(d.getDate())}`;
}

function pgnDateToInputValue(pgnDate: string): string {
  const m = /^(\d{4})\.(\d{2})\.(\d{2})$/.exec(pgnDate);
  return m ? `${m[1]}-${m[2]}-${m[3]}` : "";
}

function inputValueToPgnDate(inputValue: string): string {
  return inputValue.replace(/-/g, ".");
}

export function SaveBasePicker({ activeBasePath, tags, onConfirm, onCancel }: SaveBasePickerProps) {
  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(activeBasePath);
  const [error, setError] = useState<string | null>(null);

  const [white, setWhite] = useState(tags.White ?? "");
  const [black, setBlack] = useState(tags.Black ?? "");
  const [whiteElo, setWhiteElo] = useState(tags.WhiteElo ?? "");
  const [blackElo, setBlackElo] = useState(tags.BlackElo ?? "");
  const [date, setDate] = useState(() => {
    const raw = tags.Date ?? "";
    return raw && !raw.includes("?") ? raw : todayPgnDate();
  });
  const [result, setResult] = useState(tags.Result ?? "*");
  const [event, setEvent] = useState(tags.Event ?? "");
  const [site, setSite] = useState(tags.Site ?? "");
  const [round, setRound] = useState(tags.Round ?? "");

  useEffect(() => {
    invoke<BaseInfo[]>("list_bases")
      .then(setBases)
      .catch((e) => setError(String(e)));
  }, []);

  async function handleCreateBase() {
    try {
      const path = await save({ filters: [{ name: "Base d'échecs", extensions: ["sqlite"] }] });
      if (!path) return;
      const info = await invoke<BaseInfo>("create_base", { name: baseNameFromPath(path), path });
      setBases((prev) => [...prev, info]);
      setSelectedPath(info.path);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleConfirm() {
    if (!selectedPath) return;
    try {
      await invoke("set_tags", {
        tags: {
          White: white,
          Black: black,
          WhiteElo: whiteElo,
          BlackElo: blackElo,
          Date: date,
          Result: result,
          Event: event,
          Site: site,
          Round: round,
        },
      });
      onConfirm(selectedPath);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Sauvegarder dans quelle base ?</h2>
        {error && <p className="error">{error}</p>}
        <div className="save-game-info">
          <label>
            Blancs
            <input value={white} onChange={(e) => setWhite(e.target.value)} />
          </label>
          <label>
            Elo Blancs
            <input value={whiteElo} onChange={(e) => setWhiteElo(e.target.value)} />
          </label>
          <label>
            Noirs
            <input value={black} onChange={(e) => setBlack(e.target.value)} />
          </label>
          <label>
            Elo Noirs
            <input value={blackElo} onChange={(e) => setBlackElo(e.target.value)} />
          </label>
          <label>
            Date
            <input
              type="date"
              value={pgnDateToInputValue(date)}
              onChange={(e) => setDate(e.target.value ? inputValueToPgnDate(e.target.value) : "")}
            />
          </label>
          <label>
            Résultat
            <select value={result} onChange={(e) => setResult(e.target.value)}>
              {RESULT_OPTIONS.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
          </label>
          <label>
            Évènement
            <input value={event} onChange={(e) => setEvent(e.target.value)} />
          </label>
          <label>
            Site
            <input value={site} onChange={(e) => setSite(e.target.value)} />
          </label>
          <label>
            Round
            <input value={round} onChange={(e) => setRound(e.target.value)} />
          </label>
        </div>
        <div className="modal-table-wrap">
          <BasesTable bases={bases} selectedPath={selectedPath} onSelect={(b) => setSelectedPath(b.path)} />
        </div>
        <div className="modal-actions">
          <button onClick={handleCreateBase}>Nouvelle base</button>
          <button onClick={onCancel}>Annuler</button>
          <button disabled={!selectedPath} onClick={handleConfirm}>
            Sauvegarder
          </button>
        </div>
      </div>
    </div>
  );
}
