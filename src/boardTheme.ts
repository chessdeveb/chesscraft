import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

export const BOARD_THEME_KEY = "boardTheme";
export const BOARD_THEMES = ["brown", "blue", "green", "grey", "wood"] as const;
export type BoardThemeName = (typeof BOARD_THEMES)[number];

function isBoardThemeName(value: string): value is BoardThemeName {
  return (BOARD_THEMES as readonly string[]).includes(value);
}

export function loadStoredBoardTheme(): BoardThemeName {
  try {
    const stored = localStorage.getItem(BOARD_THEME_KEY);
    return stored && isBoardThemeName(stored) ? stored : "brown";
  } catch {
    return "brown";
  }
}

// La couleur des cases de l'echiquier pilote l'accent de toute l'interface
// (voir tokens.css) : chaque fenetre pose `data-board` a la racine, pas
// seulement celle qui affiche l'echiquier. localStorage est partage entre
// les fenetres de l'appli (meme origine), mais pas reactif d'une fenetre a
// l'autre -- le menu natif Preferences > Couleurs diffuse donc un evenement
// global (menu-set-board-theme, deja emis pour recolorer l'echiquier lui-meme)
// que chaque fenetre ecoute ici pour rester synchronisee sans redemarrer.
export function useSyncBoardTheme() {
  useEffect(() => {
    document.documentElement.dataset.board = loadStoredBoardTheme();
    const unlistenPromise = listen<string>("menu-set-board-theme", (event) => {
      if (!isBoardThemeName(event.payload)) return;
      document.documentElement.dataset.board = event.payload;
      try {
        localStorage.setItem(BOARD_THEME_KEY, event.payload);
      } catch {
        // ignore
      }
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);
}
