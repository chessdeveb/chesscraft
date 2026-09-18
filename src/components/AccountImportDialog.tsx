import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

export interface AccountImportParams {
  username: string;
  sinceMs: number | null;
  untilMs: number | null;
  perfTypes: string[];
}

interface AccountImportDialogProps {
  /** Prefixe des cles i18n (voir locales/*.json : "lichessImport.*" ou
      "chesscomImport.*") -- seul ce qui differe entre les deux sites (titre,
      placeholder du pseudo), le reste du dialogue est identique. */
  i18nPrefix: "lichessImport" | "chesscomImport";
  /** Commande backend pour recuperer la liste des cadences proposees --
      lichess::PERF_TYPES et chess_com::PERF_TYPES valent la meme chose
      (["bullet","blitz","rapid"]), voir get_lichess_perf_types cote Rust. */
  getPerfTypesCommand: string;
  onConfirm: (params: AccountImportParams) => void;
  onCancel: () => void;
}

// "YYYY-MM-DD" (valeur d'un <input type="date">) -> epoch ms UTC, borne
// incluse : minuit pour le debut de periode, 23:59:59.999 pour la fin --
// Lichess et chess.com attendent tous deux des millisecondes epoch, pas une date.
function startOfDayMs(dateStr: string): number {
  return Date.parse(`${dateStr}T00:00:00.000Z`);
}
function endOfDayMs(dateStr: string): number {
  return Date.parse(`${dateStr}T23:59:59.999Z`);
}

export function AccountImportDialog({ i18nPrefix, getPerfTypesCommand, onConfirm, onCancel }: AccountImportDialogProps) {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const [since, setSince] = useState("");
  const [until, setUntil] = useState("");
  const [perfTypes, setPerfTypes] = useState<string[]>([]);
  const [checked, setChecked] = useState<Record<string, boolean>>({});

  useEffect(() => {
    invoke<string[]>(getPerfTypesCommand).then((types) => {
      setPerfTypes(types);
      setChecked(Object.fromEntries(types.map((p) => [p, true])));
    });
  }, [getPerfTypesCommand]);

  const selectedPerfTypes = perfTypes.filter((p) => checked[p]);
  const canConfirm = username.trim() !== "" && selectedPerfTypes.length > 0;

  function handleConfirm() {
    if (!canConfirm) return;
    onConfirm({
      username: username.trim(),
      sinceMs: since ? startOfDayMs(since) : null,
      untilMs: until ? endOfDayMs(until) : null,
      perfTypes: selectedPerfTypes,
    });
  }

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{t(`${i18nPrefix}.title`)}</h2>
        <div className="account-import-form">
          <label className="account-import-field">
            {t(`${i18nPrefix}.username`)}
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t(`${i18nPrefix}.usernamePlaceholder`)}
              autoFocus
            />
          </label>
          <div className="account-import-dates">
            <label className="account-import-field">
              {t("accountImport.since")}
              <input type="date" value={since} onChange={(e) => setSince(e.target.value)} max={until || undefined} />
            </label>
            <label className="account-import-field">
              {t("accountImport.until")}
              <input type="date" value={until} onChange={(e) => setUntil(e.target.value)} min={since || undefined} />
            </label>
          </div>
          <fieldset className="account-import-perf-types">
            <legend>{t("accountImport.perfTypes")}</legend>
            {perfTypes.map((perf) => (
              <label key={perf} className="account-import-perf-type">
                <input
                  type="checkbox"
                  checked={checked[perf] ?? false}
                  onChange={(e) => setChecked((prev) => ({ ...prev, [perf]: e.target.checked }))}
                />
                {t(`accountImport.perf.${perf}`)}
              </label>
            ))}
          </fieldset>
        </div>
        <div className="modal-actions">
          <button onClick={onCancel}>{t("common.cancel")}</button>
          <button disabled={!canConfirm} onClick={handleConfirm}>
            {t("common.next")}
          </button>
        </div>
      </div>
    </div>
  );
}
