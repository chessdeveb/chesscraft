import { useCallback, useMemo, useState } from "react";

export type SortDir = "asc" | "desc";
export type ClientSort<K extends string> = { column: K; dir: SortDir } | null;

// Comparaison de texte "humaine" : ignore casse et accents, nombres dans
// l'ordre naturel ("base2" avant "base10").
export const textCollator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

export const compareText = (a: string | null | undefined, b: string | null | undefined) => textCollator.compare(a ?? "", b ?? "");

// Pour garder les valeurs absentes en dernier dans les deux sens, passer
// `isMissing` a useClientSort (une date ou une case vide n'ont rien a faire
// en tete d'un tri croissant).
export const compareNumber = (a: number | null | undefined, b: number | null | undefined) => (a ?? 0) - (b ?? 0);

export type Comparators<T, K extends string> = Record<K, (a: T, b: T) => number>;

// Tri cote interface des tableaux deja entierement charges (bases, coups de
// l'explorateur, parties d'un fichier PGN, revue des mises a jour...) -- les
// grandes listes paginees, elles, sont triees en SQL cote Rust. Meme
// comportement que leurs en-tetes : premier clic croissant, second clic
// decroissant, fleche sur la colonne active. Sans tri choisi, l'ordre
// d'origine est garde ; a egalite, aussi (Array.prototype.sort est stable).
// `comparators` doit etre stable (constante de module ou useMemo).
export function useClientSort<T, K extends string>(rows: T[], comparators: Comparators<T, K>, isMissing?: (row: T, column: K) => boolean) {
  const [sort, setSort] = useState<ClientSort<K>>(null);

  const sorted = useMemo(() => {
    if (!sort) return rows;
    const compare = comparators[sort.column];
    const sign = sort.dir === "asc" ? 1 : -1;
    return [...rows].sort((a, b) => {
      if (isMissing) {
        const ma = isMissing(a, sort.column);
        const mb = isMissing(b, sort.column);
        if (ma !== mb) return ma ? 1 : -1;
      }
      return sign * compare(a, b);
    });
  }, [rows, comparators, sort, isMissing]);

  const toggleSort = useCallback(
    (column: K) => setSort((prev) => (prev?.column === column ? { column, dir: prev.dir === "asc" ? "desc" : "asc" } : { column, dir: "asc" })),
    []
  );

  const sortArrow = useCallback((column: K) => (sort?.column === column ? (sort.dir === "asc" ? "▲" : "▼") : ""), [sort]);

  return { sorted, sort, toggleSort, sortArrow };
}
