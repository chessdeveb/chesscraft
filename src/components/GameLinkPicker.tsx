import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import type { OpenTabRef } from "../types";
import { useAutoDismissError } from "../useAutoDismissError";

interface GameRef {
  path: string;
  id: number;
}

interface ResolvedTab extends OpenTabRef {
  ref: GameRef;
}

interface GameLinkPickerProps {
  // Seules cibles possibles pour une citation (retour utilisateur : pas de
  // recherche libre dans toutes les bases, uniquement ce qui est deja
  // ouvert dans cette fenetre) -- voir App.tsx (otherTabs) et
  // get_active_game_ref cote backend.
  otherTabs: OpenTabRef[];
  onCancel: () => void;
  // label : titre de l'onglet choisi, insere tel quel dans la balise --
  // modifiable ensuite par l'utilisateur directement dans le commentaire.
  onConfirm: (path: string, id: number, label: string) => void;
}

// Citation d'une partie dans un commentaire (voir Notation.tsx) : liste des
// AUTRES onglets deja ouverts dans cette fenetre Partie. Un onglet qui n'a
// pas encore de base+id (nouvelle partie jamais sauvegardee, preparation
// adversaire) n'a rien a citer -- get_active_game_ref renvoie null pour lui,
// il est alors simplement absent de la liste plutot que propose puis en echec.
export function GameLinkPicker({ otherTabs, onCancel, onConfirm }: GameLinkPickerProps) {
  const { t } = useTranslation();
  const [resolved, setResolved] = useState<ResolvedTab[] | null>(null);
  const [error, setError] = useAutoDismissError();

  useEffect(() => {
    Promise.all(
      otherTabs.map((tab) =>
        invoke<GameRef | null>("get_active_game_ref", { tabId: tab.id })
          .then((ref): ResolvedTab | null => (ref ? { ...tab, ref } : null))
          .catch((e) => {
            setError(String(e));
            return null;
          })
      )
    ).then((tabs) => setResolved(tabs.filter((t): t is ResolvedTab => t !== null)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{t("gameLink.pickTitle")}</h2>
        {error && <p className="error">{error}</p>}
        {resolved === null ? (
          <p className="status-message">{t("common.loading")}</p>
        ) : resolved.length === 0 ? (
          <p className="notation-empty">{t("gameLink.noOtherTabs")}</p>
        ) : (
          <ul className="game-link-tab-list">
            {resolved.map((tab) => (
              <li key={tab.id}>
                <button onClick={() => onConfirm(tab.ref.path, tab.ref.id, tab.title)}>{tab.title}</button>
              </li>
            ))}
          </ul>
        )}
        <div className="modal-actions">
          <button onClick={onCancel}>{t("common.cancel")}</button>
        </div>
      </div>
    </div>
  );
}
