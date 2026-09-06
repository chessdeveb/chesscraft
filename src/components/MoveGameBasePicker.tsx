import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { BasesTable } from "./BasesTable";
import { useAutoDismissError } from "../useAutoDismissError";
import type { BaseInfo } from "../types";

interface MoveGameBasePickerProps {
  excludePath: string;
  // "choose" propose les deux boutons (bouton rapide sur une ligne), "move"
  // et "copy" viennent du menu contextuel ou l'action est deja decidee.
  mode: "choose" | "move" | "copy";
  count: number;
  onConfirm: (path: string, copy: boolean) => void;
  onCancel: () => void;
}

export function MoveGameBasePicker({ excludePath, mode, count, onConfirm, onCancel }: MoveGameBasePickerProps) {
  const { t } = useTranslation();
  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [error, setError] = useAutoDismissError();

  useEffect(() => {
    invoke<BaseInfo[]>("list_bases")
      .then((list) => setBases(list.filter((b) => b.path !== excludePath && !b.missing)))
      .catch((e) => setError(String(e)));
  }, [excludePath]);

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{count > 1 ? t("gamesTable.moveGamesTitle", { count }) : t("gamesTable.moveGameTitle")}</h2>
        {error && <p className="error">{error}</p>}
        <div className="modal-table-wrap">
          <BasesTable bases={bases} selectedPath={selectedPath} onSelect={(b) => setSelectedPath(b.path)} />
        </div>
        <div className="modal-actions">
          <button onClick={onCancel}>{t("common.cancel")}</button>
          {mode !== "move" && (
            <button disabled={!selectedPath} onClick={() => selectedPath && onConfirm(selectedPath, true)}>
              {t("gamesTable.copyGame")}
            </button>
          )}
          {mode !== "copy" && (
            <button disabled={!selectedPath} onClick={() => selectedPath && onConfirm(selectedPath, false)}>
              {t("gamesTable.moveGame")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
