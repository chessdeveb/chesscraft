import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";

// Position et taille d'une nouvelle fenetre (Partie, Aide, editeur de
// position...) pour qu'elle s'ouvre ENTIEREMENT visible. Sans x/y, Windows
// place lui-meme la fenetre en cascade a partir de celle qui la cree : si
// l'Accueil est bas sur l'ecran, la fenetre Partie (1100x750) debordait sous
// le bord de l'ecran, barre des taches comprise, et son bas restait cache.
//
// On la decale legerement de la fenetre appelante (effet de cascade), puis on
// la ramene dans la zone utile du moniteur de l'appelante (hors barre des
// taches) ; si la zone utile est plus petite que la fenetre, on reduit celle-ci.

/** Decalage par rapport a la fenetre appelante (pixels logiques). */
const CASCADE_OFFSET = 32;
/** Hauteur de barre de titre et bordures, que width/height (taille interieure
    demandee) n'incluent pas : a reserver en plus pour que le bas soit visible. */
const FRAME_HEIGHT = 48;
const FRAME_WIDTH = 16;

export interface WindowPlacement {
  x?: number;
  y?: number;
  width: number;
  height: number;
}

export async function visiblePlacement(width: number, height: number): Promise<WindowPlacement> {
  try {
    const monitor = await currentMonitor();
    if (!monitor) return { width, height };
    const scale = monitor.scaleFactor;
    // workArea exclut la barre des taches ; replie sur le moniteur entier
    const area = monitor.workArea ?? { position: monitor.position, size: monitor.size };
    const areaPos = area.position.toLogical(scale);
    const areaSize = area.size.toLogical(scale);

    const fittedWidth = Math.min(width, Math.max(320, areaSize.width - FRAME_WIDTH));
    const fittedHeight = Math.min(height, Math.max(240, areaSize.height - FRAME_HEIGHT));

    const origin = (await getCurrentWindow().outerPosition()).toLogical(scale);
    const maxX = areaPos.x + areaSize.width - (fittedWidth + FRAME_WIDTH);
    const maxY = areaPos.y + areaSize.height - (fittedHeight + FRAME_HEIGHT);
    const x = Math.max(areaPos.x, Math.min(origin.x + CASCADE_OFFSET, maxX));
    const y = Math.max(areaPos.y, Math.min(origin.y + CASCADE_OFFSET, maxY));
    return { x: Math.round(x), y: Math.round(y), width: Math.round(fittedWidth), height: Math.round(fittedHeight) };
  } catch (e) {
    // placement impossible (API indisponible) : comportement d'avant
    console.warn("placement de fenêtre :", e);
    return { width, height };
  }
}
