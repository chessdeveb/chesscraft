import { Fragment, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useSyncLanguage } from "./i18n";
import { useSyncBoardTheme } from "./boardTheme";
import { useSyncColorScheme } from "./colorScheme";
import {
  DEFAULT_SHORTCUTS,
  SHORTCUT_GROUPS,
  actionForCombo,
  comboFromEvent,
  formatCombo,
  isReservedEvent,
  saveShortcuts,
  useAnnotationShortcuts,
  type ShortcutMap,
} from "./annotationShortcuts";
import "./App.css";

const ACTION_LABEL_KEYS = new Map(
  SHORTCUT_GROUPS.flatMap((g) => g.actions.map((a) => [a.id, a.labelKey] as const))
);

/** Preferences > Raccourcis d'annotation : une ligne par entree de la barre
    d'annotation, groupees comme ses boutons. Clic sur le raccourci, puis la
    combinaison voulue ; chaque changement est enregistre aussitot et
    s'applique a toutes les fenetres Partie ouvertes (voir
    annotationShortcuts.ts). */
export function ShortcutsWindow() {
  useSyncLanguage();
  useSyncBoardTheme();
  useSyncColorScheme();
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const shortcuts = useAnnotationShortcuts();
  const [capturing, setCapturing] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Petite fenetre utilitaire : pas de menu natif (voir AboutWindow.tsx).
  useEffect(() => {
    invoke("remove_window_menu").catch(() => {});
  }, []);

  useEffect(() => {
    if (!capturing) return;
    function onKeyDown(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        setCapturing(null);
        return;
      }
      if (e.repeat) return;
      const combo = comboFromEvent(e);
      if (!combo) return; // touche de modification seule : on attend la suite
      if (isReservedEvent(e)) {
        setNotice(t("shortcuts.reserved", { combo: formatCombo(combo, lang) }));
        return;
      }
      const actionId = capturing as string;
      const next: ShortcutMap = { ...shortcuts };
      const previous = actionForCombo(next, combo);
      if (previous && previous !== actionId) {
        delete next[previous];
        setNotice(
          t("shortcuts.reassigned", {
            combo: formatCombo(combo, lang),
            previous: t(ACTION_LABEL_KEYS.get(previous) ?? previous),
            current: t(ACTION_LABEL_KEYS.get(actionId) ?? actionId),
          })
        );
      } else {
        setNotice(null);
      }
      next[actionId] = combo;
      saveShortcuts(next);
      setCapturing(null);
    }
    // phase de capture : passe avant tout autre ecouteur de la fenetre
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [capturing, shortcuts, t, lang]);

  function clear(actionId: string) {
    const next: ShortcutMap = { ...shortcuts };
    delete next[actionId];
    saveShortcuts(next);
    setNotice(null);
    setCapturing(null);
  }

  function resetDefaults() {
    saveShortcuts({ ...DEFAULT_SHORTCUTS });
    setNotice(null);
    setCapturing(null);
  }

  return (
    <main className="picker-window shortcuts-window">
      <h2>{t("shortcuts.title")}</h2>
      <p className="shortcuts-intro">{t("shortcuts.intro")}</p>
      <p className="shortcuts-notice" role="status">
        {notice ?? ""}
      </p>
      <div className="shortcuts-list">
        <table className="shortcuts-table">
          <tbody>
            {SHORTCUT_GROUPS.map((group) => (
              <Fragment key={group.id}>
                <tr className="shortcuts-group-row">
                  <th colSpan={4} scope="colgroup">
                    {t(group.titleKey)}
                  </th>
                </tr>
                {group.actions.map((action) => {
                  const combo = shortcuts[action.id];
                  const isCapturing = capturing === action.id;
                  return (
                    <tr key={action.id}>
                      <td className="shortcuts-symbol">{action.symbol}</td>
                      <td className="shortcuts-label">
                        {t(action.labelKey)}
                        {group.autoSide && <span className="shortcuts-auto-side"> · {t("shortcuts.autoSide")}</span>}
                      </td>
                      <td className="shortcuts-combo">
                        <button
                          type="button"
                          className={"shortcuts-capture" + (isCapturing ? " shortcuts-capture--active" : "")}
                          onClick={() => {
                            setNotice(null);
                            setCapturing(isCapturing ? null : action.id);
                          }}
                        >
                          {isCapturing ? t("shortcuts.press") : combo ? <kbd>{formatCombo(combo, lang)}</kbd> : t("shortcuts.none")}
                        </button>
                      </td>
                      <td className="shortcuts-clear">
                        {combo && (
                          <button
                            type="button"
                            title={t("shortcuts.clear")}
                            aria-label={t("shortcuts.clear")}
                            onClick={() => clear(action.id)}
                          >
                            ✕
                          </button>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </Fragment>
            ))}
          </tbody>
        </table>
      </div>
      <div className="modal-actions">
        <button type="button" onClick={resetDefaults}>
          {t("shortcuts.reset")}
        </button>
      </div>
    </main>
  );
}
