import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

export const COLOR_SCHEME_KEY = "colorScheme";
export const COLOR_SCHEMES = ["system", "light", "dark"] as const;
export type ColorSchemeName = (typeof COLOR_SCHEMES)[number];

function isColorSchemeName(value: string): value is ColorSchemeName {
  return (COLOR_SCHEMES as readonly string[]).includes(value);
}

export function loadStoredColorScheme(): ColorSchemeName {
  try {
    const stored = localStorage.getItem(COLOR_SCHEME_KEY);
    return stored && isColorSchemeName(stored) ? stored : "system";
  } catch {
    return "system";
  }
}

// "systeme" = pas d'attribut du tout : tokens.css suit alors uniquement
// prefers-color-scheme. "light"/"dark" posent data-theme, que tokens.css
// fait gagner sur la preference systeme (voir le garde-fou
// :not([data-theme="light"]) sur son bloc media, et le bloc
// [data-theme="dark"] dedie). Meme mecanisme de diffusion inter-fenetres
// que useSyncBoardTheme (voir boardTheme.ts).
export function useSyncColorScheme() {
  useEffect(() => {
    const apply = (scheme: ColorSchemeName) => {
      if (scheme === "system") {
        delete document.documentElement.dataset.theme;
      } else {
        document.documentElement.dataset.theme = scheme;
      }
    };
    apply(loadStoredColorScheme());
    const unlistenPromise = listen<string>("menu-set-color-scheme", (event) => {
      if (!isColorSchemeName(event.payload)) return;
      apply(event.payload);
      try {
        localStorage.setItem(COLOR_SCHEME_KEY, event.payload);
      } catch {
        // ignore
      }
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);
}
