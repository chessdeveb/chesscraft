import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useTranslation } from "react-i18next";
import { useSyncLanguage } from "./i18n";
import { useSyncBoardTheme } from "./boardTheme";
import { useSyncColorScheme } from "./colorScheme";
// Texte officiel de la licence des polices IBM Plex (SIL OFL 1.1), embarque
// dans l'appli : la licence exige qu'il accompagne chaque copie des polices,
// y compris celles integrees a l'installateur (voir src/fonts/OFL.txt).
import fontsLicenseText from "./fonts/OFL.txt?raw";
import "./App.css";

const SOURCE_URL = "https://github.com/chessdeveb/chesscraft";

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up-to-date" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; percent: number | null }
  | { kind: "error"; message: string };

export function AboutWindow() {
  useSyncLanguage();
  useSyncBoardTheme();
  useSyncColorScheme();
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  // Petite fenetre utilitaire : pas de menu natif souhaite (voir
  // lib.rs::remove_window_menu -- sous Windows, une fenetre creee sans menu
  // explicite peut malgre tout heriter du menu partage de l'appli, de facon
  // intermittente).
  useEffect(() => {
    invoke("remove_window_menu").catch(() => {});
  }, []);

  async function handleCheck() {
    setState({ kind: "checking" });
    try {
      const update = await check();
      setState(update ? { kind: "available", update } : { kind: "up-to-date" });
    } catch (e) {
      setState({ kind: "error", message: String(e) });
    }
  }

  async function handleInstall(update: Update) {
    // Sur Windows, downloadAndInstall lance l'installeur MSI puis quitte
    // l'appli elle-meme -- pas besoin de relancer manuellement ensuite.
    let total: number | null = null;
    let received = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
          received = 0;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          setState({ kind: "downloading", percent: total ? Math.round((received / total) * 100) : null });
        }
      });
    } catch (e) {
      setState({ kind: "error", message: String(e) });
    }
  }

  return (
    <main className="picker-window">
      <h2>{t("about.title")}</h2>
      <p>{t("about.version", { version })}</p>

      {state.kind === "idle" && <button onClick={handleCheck}>{t("about.checkUpdate")}</button>}
      {state.kind === "checking" && <p>{t("about.checking")}</p>}
      {state.kind === "up-to-date" && (
        <>
          <p>{t("about.upToDate")}</p>
          <button onClick={handleCheck}>{t("about.checkUpdate")}</button>
        </>
      )}
      {state.kind === "available" && (
        <>
          <p>{t("about.updateAvailable", { version: state.update.version })}</p>
          {state.update.body && <p className="about-release-notes">{state.update.body}</p>}
          <button onClick={() => handleInstall(state.update)}>{t("about.installUpdate")}</button>
        </>
      )}
      {state.kind === "downloading" && (
        <p>{state.percent !== null ? t("about.downloadingPercent", { percent: state.percent }) : t("about.downloading")}</p>
      )}
      {state.kind === "error" && (
        <>
          <p className="error">{state.message}</p>
          <button onClick={handleCheck}>{t("about.checkUpdate")}</button>
        </>
      )}

      {/* Avis de licence : GPL-3.0 de l'appli (que l'interface doit
          afficher) et licence des polices embarquees. */}
      <section className="about-licenses">
        <h3>{t("about.licensesTitle")}</h3>
        <p>
          {t("about.appLicense")}{" "}
          <a
            href={SOURCE_URL}
            onClick={(e) => {
              e.preventDefault();
              openUrl(SOURCE_URL).catch((err) => console.error("openUrl", err));
            }}
          >
            {t("about.sourceCode")}
          </a>
        </p>
        <details>
          <summary>{t("about.fontsLicense")}</summary>
          <pre className="about-license-text">{fontsLicenseText}</pre>
        </details>
      </section>
    </main>
  );
}
