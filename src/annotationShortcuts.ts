import { useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { NAG_CLUSTERS, type NagCluster, type NagChoice } from "./nag";

// Raccourcis clavier des annotations (Preferences > Raccourcis d'annotation,
// voir ShortcutsWindow.tsx), appliques au coup courant de l'onglet Partie
// actif (voir GameTab.tsx). Une "action" par entree de la barre
// d'annotation : un des boutons d'edition (promouvoir/supprimer la variante,
// supprimer les coups suivants/precedents ou toutes les annotations), un
// code NAG precis, ou
// pour les clusters autoSide (espace, developpement...) le cluster entier --
// le code Blancs ou Noirs est choisi selon le coup, exactement comme le
// bouton de la barre.
//
// Une combinaison est stockee sous forme de texte, modificateurs dans un
// ordre fixe puis la touche PHYSIQUE (KeyboardEvent.code) :
// "Ctrl+Alt+Digit1". La touche physique plutot que le caractere produit :
// sur un clavier AZERTY, Ctrl+1 produit "&" (et Ctrl+Alt+2, c'est-a-dire
// AltGr+2, la touche morte "~") -- seule la position de la touche reste
// stable quelle que soit la disposition.

export const SHORTCUTS_KEY = "annotationShortcuts";
const CHANGED_EVENT = "annotation-shortcuts-changed";

export type ShortcutMap = Record<string, string>;

export interface ShortcutAction {
  id: string;
  symbol: string;
  labelKey: string;
}

export interface ShortcutGroup {
  id: string;
  titleKey: string;
  /** Cluster autoSide : une seule action, Blancs ou Noirs selon le coup. */
  autoSide: boolean;
  actions: ShortcutAction[];
}

function nagAction(n: NagChoice): ShortcutAction {
  return { id: `nag:${n.code}`, symbol: n.symbol, labelKey: n.labelKey };
}

function nagGroup(cluster: NagCluster): ShortcutGroup {
  return {
    id: cluster.id,
    titleKey: cluster.titleKey,
    autoSide: !!cluster.autoSide,
    actions: cluster.autoSide
      ? [{ id: `cluster:${cluster.id}`, symbol: cluster.symbol, labelKey: cluster.titleKey }]
      : cluster.entries.map(nagAction),
  };
}

/** Les boutons d'edition a gauche de la barre d'annotation (voir
    Notation.tsx, memes symboles et libelles). */
export type EditAction =
  | "promoteVariation"
  | "deleteVariation"
  | "deleteFollowingMoves"
  | "deletePreviousMoves"
  | "clearAnnotations";

export const EDIT_ACTION_SYMBOLS: Record<EditAction, string> = {
  promoteVariation: "⬆️",
  deleteVariation: "🗑️",
  deleteFollowingMoves: "✂⇥",
  deletePreviousMoves: "⇤✂",
  clearAnnotations: "🧹",
};

const EDIT_GROUP: ShortcutGroup = {
  id: "edit",
  titleKey: "shortcuts.group.edit",
  autoSide: false,
  actions: (Object.keys(EDIT_ACTION_SYMBOLS) as EditAction[]).map((action) => ({
    id: `edit:${action}`,
    symbol: EDIT_ACTION_SYMBOLS[action],
    labelKey: `notation.${action}`,
  })),
};

export const SHORTCUT_GROUPS: ShortcutGroup[] = [EDIT_GROUP, ...NAG_CLUSTERS.map(nagGroup)];

const ACTION_IDS = new Set(SHORTCUT_GROUPS.flatMap((g) => g.actions.map((a) => a.id)));

export const DEFAULT_SHORTCUTS: ShortcutMap = {
  // qualite du coup : ! ? !! ?? !? ?!
  "nag:1": "Ctrl+Digit1",
  "nag:2": "Ctrl+Digit2",
  "nag:3": "Ctrl+Digit3",
  "nag:4": "Ctrl+Digit4",
  "nag:5": "Ctrl+Digit5",
  "nag:6": "Ctrl+Digit6",
  // evaluation : = ∞ ⩲ ⩱ ± ∓ +− −+
  "nag:10": "Ctrl+Alt+Digit1",
  "nag:13": "Ctrl+Alt+Digit2",
  "nag:14": "Ctrl+Alt+Digit3",
  "nag:15": "Ctrl+Alt+Digit4",
  "nag:16": "Ctrl+Alt+Digit5",
  "nag:17": "Ctrl+Alt+Digit6",
  "nag:18": "Ctrl+Alt+Digit7",
  "nag:19": "Ctrl+Alt+Digit8",
};

/** Raccourcis en vigueur : ceux enregistres par l'utilisateur (la table
    complete, y compris les raccourcis effaces), sinon les valeurs par
    defaut. */
export function loadShortcuts(): ShortcutMap {
  try {
    const raw = localStorage.getItem(SHORTCUTS_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (parsed && typeof parsed === "object") {
        const map: ShortcutMap = {};
        for (const [id, combo] of Object.entries(parsed as Record<string, unknown>)) {
          if (ACTION_IDS.has(id) && typeof combo === "string" && combo) map[id] = combo;
        }
        return map;
      }
    }
  } catch {
    // ignore : valeurs par defaut
  }
  return { ...DEFAULT_SHORTCUTS };
}

/** Enregistre la table et previent toutes les fenetres (localStorage est
    partage entre elles, mais pas reactif d'une fenetre a l'autre). */
export function saveShortcuts(map: ShortcutMap) {
  try {
    localStorage.setItem(SHORTCUTS_KEY, JSON.stringify(map));
  } catch {
    // ignore
  }
  emit(CHANGED_EVENT).catch(() => {});
}

/** Table courante, tenue a jour quand elle change dans une autre fenetre. */
export function useAnnotationShortcuts(): ShortcutMap {
  const [map, setMap] = useState<ShortcutMap>(loadShortcuts);
  useEffect(() => {
    const unlistenPromise = listen(CHANGED_EVENT, () => setMap(loadShortcuts()));
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);
  return map;
}

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
  "CapsLock",
  "NumLock",
]);

/** Combinaison d'un evenement clavier, ou null pour une touche de
    modification seule (on attend la touche qui l'accompagne). Les chiffres
    du pave numerique valent ceux de la rangee du haut. */
export function comboFromEvent(e: KeyboardEvent): string | null {
  if (!e.code || MODIFIER_CODES.has(e.code)) return null;
  const numpadDigit = /^Numpad(\d)$/.exec(e.code);
  const code = numpadDigit ? `Digit${numpadDigit[1]}` : e.code;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");
  parts.push(code);
  return parts.join("+");
}

// Touches deja prises par l'application : navigation dans la partie et
// listes (fleches, Entree, Echap), Aide (F1), et les raccourcis du menu
// Fichier (Ctrl+N/P/O/I/E/S, reconnus sur le caractere produit, voir
// HomeWindow.tsx et GameTab.tsx).
const RESERVED_CODES = new Set([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Enter",
  "NumpadEnter",
  "Escape",
  "Tab",
  "F1",
]);
const RESERVED_CTRL_KEYS = new Set(["n", "p", "o", "i", "e", "s"]);

export function isReservedEvent(e: KeyboardEvent): boolean {
  if (RESERVED_CODES.has(e.code)) return true;
  return (e.ctrlKey || e.metaKey) && !e.altKey && RESERVED_CTRL_KEYS.has(e.key.toLowerCase());
}

/** Cible en cours de saisie (commentaire, champ de recherche...) : les
    raccourcis d'annotation ne doivent pas y voler les touches. */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

// Libelle des touches selon la disposition du clavier (AZERTY : KeyQ -> "A").
// API de Chromium (WebView2), asynchrone : chargee une fois, repli sur un
// libelle QWERTY tant qu'elle n'a pas repondu.
let layoutMap: Map<string, string> | null = null;
const keyboardApi = (navigator as Navigator & { keyboard?: { getLayoutMap?: () => Promise<Map<string, string>> } })
  .keyboard;
keyboardApi
  ?.getLayoutMap?.()
  .then((m) => {
    layoutMap = new Map(m);
  })
  .catch(() => {});

const NAMED_KEYS: Record<string, { fr: string; en: string }> = {
  Space: { fr: "Espace", en: "Space" },
  Backspace: { fr: "Retour arrière", en: "Backspace" },
  Delete: { fr: "Suppr", en: "Delete" },
  Insert: { fr: "Inser", en: "Insert" },
  Home: { fr: "Début", en: "Home" },
  End: { fr: "Fin", en: "End" },
  PageUp: { fr: "Page préc.", en: "Page Up" },
  PageDown: { fr: "Page suiv.", en: "Page Down" },
};

function keyLabel(code: string, lang: string): string {
  const en = lang === "en";
  const digit = /^Digit(\d)$/.exec(code);
  if (digit) return digit[1];
  if (/^F\d{1,2}$/.test(code)) return code;
  if (NAMED_KEYS[code]) return en ? NAMED_KEYS[code].en : NAMED_KEYS[code].fr;
  const fromLayout = layoutMap?.get(code);
  if (fromLayout && fromLayout.trim()) return fromLayout.toUpperCase();
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const numpad = /^Numpad(.+)$/.exec(code);
  if (numpad) return (en ? "Num " : "Pavé ") + numpad[1];
  return code;
}

/** "Ctrl+Alt+Digit1" -> "Ctrl+Alt+1" ("Maj" pour Shift en francais). */
export function formatCombo(combo: string, lang: string): string {
  const parts = combo.split("+");
  const code = parts.pop() ?? "";
  const mods = parts.map((m) => {
    if (m === "Shift") return lang === "en" ? "Shift" : "Maj";
    if (m === "Meta") return "Win";
    return m;
  });
  return [...mods, keyLabel(code, lang)].join("+");
}

/** Code NAG a basculer pour une action, selon le camp qui a joue le coup
    courant (clusters autoSide seulement). */
export function nagCodeForAction(actionId: string, moverWhite: boolean): number | null {
  const nag = /^nag:(\d+)$/.exec(actionId);
  if (nag) return Number(nag[1]);
  const cluster = /^cluster:(.+)$/.exec(actionId);
  if (cluster) {
    const found = NAG_CLUSTERS.find((c) => c.id === cluster[1] && c.autoSide);
    if (found) return (moverWhite ? found.entries[0] : found.entries[1]).code;
  }
  return null;
}

/** Bouton d'edition (promouvoir, supprimer...) designe par une action, ou
    null si l'action est une annotation. */
export function editActionFor(actionId: string): EditAction | null {
  const edit = /^edit:(.+)$/.exec(actionId);
  return edit && edit[1] in EDIT_ACTION_SYMBOLS ? (edit[1] as EditAction) : null;
}

/** Action associee a une combinaison, s'il y en a une. */
export function actionForCombo(map: ShortcutMap, combo: string): string | null {
  for (const [id, c] of Object.entries(map)) {
    if (c === combo) return id;
  }
  return null;
}
