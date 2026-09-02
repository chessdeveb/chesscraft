import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { themeDark, themeLight, type DockviewApi, type DockviewTheme } from "dockview-react";

// <DockviewReact> ne recoit son theme (couleurs, classe CSS posee sur son
// element .dv-shell interne) que via la prop `theme` -- une classe posee sur
// un <div> englobant n'atteint jamais cet element interne. Sans cette prop,
// dockview retombe sur son theme "abyss" par defaut (bleu nuit fonce), ce qui
// explique un ecran qui ne bouge pas malgre les jetons de couleur de l'appli.
export function useDockviewTheme(): DockviewTheme {
  const [dark, setDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  return dark ? themeDark : themeLight;
}

function loadDockviewLayout(key: string): unknown | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

// Tente de restaurer une disposition sauvegardee (taille/position/ordre des
// panneaux) ; renvoie true si ca a reussi, l'appelant n'a alors pas besoin de
// reconstruire les panneaux par defaut. Un echec (rien de sauvegarde encore,
// ou schema perime apres l'ajout d'un panneau depuis) retombe silencieusement
// sur la construction par defaut de l'appelant.
export function tryRestoreDockviewLayout(api: DockviewApi, key: string): boolean {
  const saved = loadDockviewLayout(key);
  if (!saved) return false;
  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    api.fromJSON(saved as any);
    return true;
  } catch {
    return false;
  }
}

// Remet a jour les cases "Affichage > panneau" du menu natif d'apres l'etat
// reel du dockview : sans ca, fermer un panneau via son propre bouton (pas
// via le menu) laisse la case cochee, et la recliquer rouvre alors le
// panneau au lieu de "finir" de le fermer -- tres deroutant. `panelIds` sont
// les ids de panneaux dockview (ex. "notation"), prefixes en "panel_notation"
// cote menu natif (voir build_menu). Le backend sait, d'apres la fenetre
// appelante, quelle case mettre a jour (voir set_panel_checked).
export function syncPanelCheckedStates(api: DockviewApi, panelIds: string[]): void {
  for (const id of panelIds) {
    invoke("set_panel_checked", { id: `panel_${id}`, checked: !!api.getPanel(id) }).catch(() => {});
  }
}

// Sauvegarde la disposition a chaque changement (redimensionnement,
// deplacement, fermeture/ouverture d'un panneau...), avec un debounce pour
// eviter d'ecrire a chaque frame pendant un glisser-deposer -- resynchronise
// au passage les cases du menu natif si `panelIds` est fourni. Renvoie une
// fonction de nettoyage a appeler au demontage.
export function persistDockviewLayout(api: DockviewApi, key: string, panelIds?: string[]): () => void {
  let timeout: ReturnType<typeof setTimeout> | null = null;
  const disposable = api.onDidLayoutChange(() => {
    if (timeout) clearTimeout(timeout);
    timeout = setTimeout(() => {
      try {
        localStorage.setItem(key, JSON.stringify(api.toJSON()));
      } catch {
        // quota depassee ou similaire : pas critique, on ignore
      }
      if (panelIds) syncPanelCheckedStates(api, panelIds);
    }, 300);
  });
  return () => {
    if (timeout) clearTimeout(timeout);
    disposable.dispose();
  };
}
