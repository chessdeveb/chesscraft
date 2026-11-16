import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useTranslation } from "react-i18next";

type PromptState =
  | { kind: "available"; update: Update }
  | { kind: "downloading"; update: Update; percent: number | null }
  | { kind: "error"; update: Update; message: string };

// Une seule verification par lancement de l'application, quel que soit le
// nombre de montages du composant (StrictMode le monte deux fois en
// developpement).
let checkStarted = false;

/** Verification silencieuse d'une nouvelle version a l'ouverture du
    programme (fenetre Accueil) : rien ne s'affiche tant qu'il n'y en a pas,
    ni si la verification echoue (pas de reseau, GitHub injoignable...) --
    Aide > A propos reste la pour une verification a la demande, avec ses
    messages d'erreur. Si une version existe, une boite propose de
    l'installer ; "Plus tard" la referme jusqu'au prochain lancement. */
export function UpdatePrompt() {
  const { t } = useTranslation();
  const [state, setState] = useState<PromptState | null>(null);
  const [currentVersion, setCurrentVersion] = useState("");

  useEffect(() => {
    if (checkStarted) return;
    checkStarted = true;
    check()
      .then((update) => {
        if (update) setState({ kind: "available", update });
      })
      .catch((e) => console.warn("vérification de mise à jour au démarrage :", e));
    getVersion()
      .then(setCurrentVersion)
      .catch(() => {});
  }, []);

  if (!state) return null;
  const { update } = state;

  async function install() {
    // Sous Windows, downloadAndInstall lance l'installateur MSI puis quitte
    // l'application elle-meme (voir AboutWindow.tsx).
    let total: number | null = null;
    let received = 0;
    setState({ kind: "downloading", update, percent: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
          received = 0;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          setState({ kind: "downloading", update, percent: total ? Math.round((received / total) * 100) : null });
        }
      });
    } catch (e) {
      setState({ kind: "error", update, message: String(e) });
    }
  }

  return (
    <div className="modal-overlay">
      <div className="modal update-prompt">
        <h2>{t("updatePrompt.title")}</h2>
        <p>
          {currentVersion
            ? t("updatePrompt.available", { version: update.version, current: currentVersion })
            : t("about.updateAvailable", { version: update.version })}
        </p>
        {update.body && <p className="about-release-notes update-prompt-notes">{update.body}</p>}

        {state.kind === "available" && <p className="update-prompt-hint">{t("updatePrompt.restartHint")}</p>}
        {state.kind === "downloading" && (
          <p className="status-message">
            {state.percent !== null ? t("about.downloadingPercent", { percent: state.percent }) : t("about.downloading")}
          </p>
        )}
        {state.kind === "error" && <p className="error">{t("updatePrompt.failed", { message: state.message })}</p>}

        {state.kind !== "downloading" && (
          <div className="modal-actions">
            <button type="button" onClick={() => setState(null)}>
              {t("updatePrompt.later")}
            </button>
            <button type="button" className="update-prompt-primary" onClick={install}>
              {state.kind === "error" ? t("updatePrompt.retry") : t("updatePrompt.install")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
