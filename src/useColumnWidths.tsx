import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";

export interface ColumnSpec {
  key: string;
  default: number;
  min?: number;
}

const DEFAULT_MIN_WIDTH = 32;

function loadWidths(storageKey: string, columns: ColumnSpec[]): Record<string, number> {
  const widths: Record<string, number> = {};
  for (const c of columns) widths[c.key] = c.default;
  try {
    const raw = localStorage.getItem(`colWidths:${storageKey}`);
    if (raw) {
      const saved = JSON.parse(raw) as Record<string, unknown>;
      for (const c of columns) {
        const v = saved[c.key];
        if (typeof v === "number" && Number.isFinite(v)) widths[c.key] = v;
      }
    }
  } catch {
    // stockage prive/quota/JSON invalide : retombe sur les largeurs par defaut
  }
  return widths;
}

// Largeurs de colonnes redimensionnables a la souris, retenues d'une session
// a l'autre (localStorage, par defaut cote navigateur -- voir le skill
// artifact-capabilities pour le meme principe cote artifacts : jamais
// partage entre postes, mais suffisant ici, chaque utilisateur de l'appli
// etant sur sa propre machine). `storageKey` distingue un tableau de l'autre
// (ex. "gamesTable", "playersTable") ; `columns` donne la largeur par defaut
// (et un minimum, DEFAULT_MIN_WIDTH sinon) de chaque colonne redimensionnable
// -- une colonne non listee ici (ex. celle des boutons d'action) garde sa
// largeur CSS fixe habituelle, inchangee.
export function useColumnWidths(storageKey: string, columns: ColumnSpec[]) {
  const [widths, setWidths] = useState<Record<string, number>>(() => loadWidths(storageKey, columns));
  const dragRef = useRef<{ key: string; startX: number; startWidth: number; min: number } | null>(null);

  const persist = useCallback(
    (next: Record<string, number>) => {
      try {
        localStorage.setItem(`colWidths:${storageKey}`, JSON.stringify(next));
      } catch {
        // la largeur reste correcte pour cette session, juste pas retenue
      }
    },
    [storageKey]
  );

  useEffect(() => {
    function handleMove(e: MouseEvent) {
      const drag = dragRef.current;
      if (!drag) return;
      const next = Math.max(drag.min, drag.startWidth + (e.clientX - drag.startX));
      setWidths((prev) => (prev[drag.key] === next ? prev : { ...prev, [drag.key]: next }));
    }
    function handleUp() {
      if (!dragRef.current) return;
      dragRef.current = null;
      setWidths((current) => {
        persist(current);
        return current;
      });
    }
    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseup", handleUp);
    return () => {
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseup", handleUp);
    };
  }, [persist]);

  // A poser en onMouseDown sur la poignee de la colonne `key` -- demarre le
  // glisser, stoppe la propagation pour ne pas declencher par la meme
  // occasion un clic de tri sur l'en-tete (voir SortableHeader).
  const startResize = useCallback(
    (key: string, min = DEFAULT_MIN_WIDTH) =>
      (e: ReactMouseEvent) => {
        e.preventDefault();
        e.stopPropagation();
        dragRef.current = { key, startX: e.clientX, startWidth: widths[key] ?? 80, min };
      },
    [widths]
  );

  // Somme des colonnes suivies -- a poser en style.width sur l'element
  // <table> lui-meme (voir chaque *PanelView/*Table.tsx). Indispensable :
  // table-layout:fixed avec un <table> reste a width:auto n'honore PAS une
  // largeur de colonne plus etroite que son contenu (verifie en pratique --
  // un texte en white-space:nowrap continue d'imposer sa largeur minimale
  // malgre overflow:hidden/max-width:none sur la cellule), seule une largeur
  // de TABLE explicite (pas juste "auto") force vraiment chaque colonne a
  // respecter la largeur demandee, y compris pour la retrecir en glissant.
  const totalWidth = Object.values(widths).reduce((sum, w) => sum + w, 0);

  return { widths, startResize, totalWidth };
}

// Etire un tableau a colonnes redimensionnables jusqu'a la largeur de son
// conteneur (l'element parent du <table>, observe avec ResizeObserver) :
// l'espace libre est reparti a parts egales entre `growKeys`, les autres
// colonnes gardent exactement la largeur choisie. Plus etroit que la somme
// des colonnes, rien n'est retreci : le conteneur defile horizontalement,
// comme sans ce hook. A utiliser avec useColumnWidths : `fitted` remplace
// `widths` sur les en-tetes, `tableWidth` remplace `totalWidth`, et
// `tableRef` se pose sur le <table>.
export function useFillWidth(widths: Record<string, number>, growKeys: string[]) {
  const [table, setTable] = useState<HTMLTableElement | null>(null);
  // largeur disponible, deja diminuee des marges interieures des en-tetes :
  // la largeur posee sur un <th> ne les compte pas, la colonne reelle fait
  // largeur + marges (sans ce calcul, le tableau depassait de quelques
  // pixels et faisait apparaitre un defilement horizontal)
  const [available, setAvailable] = useState(0);
  const tableRef = useCallback((node: HTMLTableElement | null) => setTable(node), []);

  useEffect(() => {
    const container = table?.parentElement;
    if (!table || !container) return;
    const update = () => {
      let padding = 0;
      table.querySelectorAll("thead th").forEach((th) => {
        const cs = getComputedStyle(th);
        padding += parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
      });
      setAvailable(container.clientWidth - padding);
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(container);
    return () => observer.disconnect();
  }, [table]);

  const total = Object.values(widths).reduce((sum, w) => sum + w, 0);
  const extra = growKeys.length > 0 ? Math.max(0, Math.floor(available - total)) : 0;
  const fitted = { ...widths };
  growKeys.forEach((key, i) => {
    const share = Math.floor(extra / growKeys.length) + (i === 0 ? extra % growKeys.length : 0);
    fitted[key] = (fitted[key] ?? 0) + share;
  });
  return { tableRef, fitted, tableWidth: total + extra };
}

// Poignee de redimensionnement : un trait fin colle au bord droit de
// l'en-tete (voir .col-resize-handle, App.css) -- l'en-tete doit etre en
// position:relative pour que ce positionnement absolu s'ancre bien sur lui,
// pas sur un ancetre plus eloigne.
export function ColumnResizeHandle({ onMouseDown }: { onMouseDown: (e: ReactMouseEvent) => void }) {
  return <span className="col-resize-handle" onMouseDown={onMouseDown} onClick={(e) => e.stopPropagation()} />;
}
