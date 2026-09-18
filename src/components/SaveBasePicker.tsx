import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { BasesTable } from "./BasesTable";
import { FideNameInput } from "./FideNameInput";
import { useAutoDismissError } from "../useAutoDismissError";
import { classifyTimeControl, type TimeControlCategory } from "../timeControl";
import type { BaseInfo } from "../types";

const TIME_CONTROL_CATEGORIES: TimeControlCategory[] = ["blitz", "rapid", "classical"];

interface SaveBasePickerProps {
  tabId: number;
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

// "?" est le marqueur PGN standard pour une valeur inconnue (voir game_tree::new,
// pgn::parse_game_at) : sans ce nettoyage il s'affiche tel quel dans les champs
// d'une partie fraichement creee ou importee sans ces informations.
function stripUnknownPlaceholder(raw: string | undefined): string {
  return raw && raw !== "?" ? raw : "";
}

export function SaveBasePicker({ tabId, activeBasePath, tags, onConfirm, onCancel }: SaveBasePickerProps) {
  const { t } = useTranslation();
  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [basesLoading, setBasesLoading] = useState(true);
  const [selectedPath, setSelectedPath] = useState<string | null>(activeBasePath);
  const [error, setError] = useAutoDismissError();

  const [white, setWhite] = useState(stripUnknownPlaceholder(tags.White));
  const [black, setBlack] = useState(stripUnknownPlaceholder(tags.Black));
  const [whiteElo, setWhiteElo] = useState(tags.WhiteElo ?? "");
  const [blackElo, setBlackElo] = useState(tags.BlackElo ?? "");
  const [date, setDate] = useState(() => {
    const raw = tags.Date ?? "";
    return raw && !raw.includes("?") ? raw : todayPgnDate();
  });
  const [result, setResult] = useState(tags.Result ?? "*");
  const [event, setEvent] = useState(stripUnknownPlaceholder(tags.Event));
  const [site, setSite] = useState(stripUnknownPlaceholder(tags.Site));
  const [round, setRound] = useState(stripUnknownPlaceholder(tags.Round));
  const [timeControl, setTimeControl] = useState(stripUnknownPlaceholder(tags.TimeControl));
  // Coche automatiquement la bonne categorie a chaque frappe (voir
  // classifyTimeControl), mais reste un choix independant ensuite : cliquer
  // sur un bouton la change sans toucher au texte tape, pour corriger un cas
  // que la formule estimerait mal sans avoir a inventer une autre cadence.
  const [category, setCategory] = useState<TimeControlCategory | null>(() => classifyTimeControl(timeControl));
  function handleTimeControlChange(value: string) {
    setTimeControl(value);
    setCategory(classifyTimeControl(value));
  }

  useEffect(() => {
    invoke<BaseInfo[]>("list_bases")
      .then(setBases)
      .catch((e) => setError(String(e)))
      .finally(() => setBasesLoading(false));
  }, []);

  async function handleCreateBase() {
    try {
      const path = await save({ filters: [{ name: t("common.chessBaseFilter"), extensions: ["sqlite"] }] });
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
        tabId,
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
          TimeControl: timeControl,
        },
      });
      onConfirm(selectedPath);
    } catch (e) {
      setError(String(e));
    }
  }

  // Ctrl+S declenche deja l'ouverture de cette fenetre (voir le raccourci
  // clavier de GameTab). Sans ce second abonnement, le rappuyer pendant
  // qu'elle est deja ouverte ne faisait rien de visible : handleSave() se
  // contente de mettre showSaveBasePicker a true, deja le cas. Le ref evite
  // de reabonner a chaque frappe dans un champ du formulaire.
  const handleConfirmRef = useRef(handleConfirm);
  handleConfirmRef.current = handleConfirm;
  useEffect(() => {
    // "menu-save-game" : un clic sur Fichier > Enregistrer pendant que la
    // fenetre est deja ouverte (l'accelerateur clavier natif ne se declenche
    // jamais sous Windows, voir HomeWindow.tsx -- keydown ci-dessous couvre
    // le cas clavier).
    const unlisten = listen("menu-save-game", () => handleConfirmRef.current());
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        handleConfirmRef.current();
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      unlisten.then((f) => f());
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{t("savePicker.title")}</h2>
        {error && <p className="error">{error}</p>}
        <div className="save-game-info">
          <label>
            {t("common.white")}
            <FideNameInput
              id="save-picker-white"
              value={white}
              onChange={setWhite}
              onSelectPlayer={(p) => {
                if (p.stdRating) setWhiteElo(String(p.stdRating));
              }}
            />
          </label>
          <label>
            {t("savePicker.whiteElo")}
            <input value={whiteElo} onChange={(e) => setWhiteElo(e.target.value)} />
          </label>
          <label>
            {t("common.black")}
            <FideNameInput
              id="save-picker-black"
              value={black}
              onChange={setBlack}
              onSelectPlayer={(p) => {
                if (p.stdRating) setBlackElo(String(p.stdRating));
              }}
            />
          </label>
          <label>
            {t("savePicker.blackElo")}
            <input value={blackElo} onChange={(e) => setBlackElo(e.target.value)} />
          </label>
          <label>
            {t("common.date")}
            <input
              type="date"
              value={pgnDateToInputValue(date)}
              onChange={(e) => setDate(e.target.value ? inputValueToPgnDate(e.target.value) : "")}
            />
          </label>
          <label>
            {t("common.result")}
            <select value={result} onChange={(e) => setResult(e.target.value)}>
              {RESULT_OPTIONS.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t("common.event")}
            <input value={event} onChange={(e) => setEvent(e.target.value)} />
          </label>
          <label>
            {t("savePicker.site")}
            <input value={site} onChange={(e) => setSite(e.target.value)} />
          </label>
          <label>
            {t("savePicker.round")}
            <input value={round} onChange={(e) => setRound(e.target.value)} />
          </label>
          <label>
            <span className="savepicker-label-with-help">
              {t("savePicker.timeControl")}
              <span className="help-bubble" title={t("savePicker.timeControlHelp")}>
                ⓘ
              </span>
            </span>
            <input
              value={timeControl}
              onChange={(e) => handleTimeControlChange(e.target.value)}
              placeholder={t("savePicker.timeControlPlaceholder")}
            />
          </label>
        </div>
        <div className="savepicker-category-row">
          {TIME_CONTROL_CATEGORIES.map((cat) => (
            <button
              key={cat}
              type="button"
              className={"savepicker-category-btn" + (category === cat ? " savepicker-category-btn--active" : "")}
              onClick={() => setCategory(cat)}
            >
              {t(`savePicker.category.${cat}`)}
            </button>
          ))}
        </div>
        <div className="modal-table-wrap">
          <BasesTable bases={bases} loading={basesLoading} selectedPath={selectedPath} onSelect={(b) => setSelectedPath(b.path)} />
        </div>
        <div className="modal-actions">
          <button onClick={handleCreateBase}>{t("common.newBase")}</button>
          <button onClick={onCancel}>{t("common.cancel")}</button>
          <button disabled={!selectedPath} onClick={handleConfirm}>
            {t("common.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
