import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { ExplorerFilterValues } from "../types";

interface ExplorerFiltersBarProps {
  filters: ExplorerFilterValues;
  onChange: (filters: ExplorerFilterValues) => void;
  onCommit: () => void;
}

const EMPTY_FILTERS: ExplorerFilterValues = { playerName: "", playerWhite: true, ratingMin: "", ratingMax: "", yearMin: "", yearMax: "" };

// Champs numeriques geres comme du texte libre (pas <input type="number">) :
// permet un champ vide pendant la frappe sans que le navigateur ne le force
// a 0 -- explorerFilterArgs (GameTab.tsx) traite "" comme "pas de filtre".
//
// Aucun champ ne declenche la moindre requete a la frappe : `onChange` ne
// touche que la saisie locale (voir explorerFiltersInput, GameTab.tsx),
// `onCommit` (Entree dans un champ, ou le bouton Rechercher) valide -- meme
// principe que les filtres avances de la fenetre Parties
// (gamesAdvancedInput/gamesAdvancedFilter, HomeDockview.tsx).
export function ExplorerFiltersBar({ filters, onChange, onCommit }: ExplorerFiltersBarProps) {
  const { t } = useTranslation();

  function set<K extends keyof ExplorerFilterValues>(key: K, value: ExplorerFilterValues[K]) {
    onChange({ ...filters, [key]: value });
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter") onCommit();
  }

  const hasActiveFilter =
    filters.playerName.trim() !== "" || filters.ratingMin !== "" || filters.ratingMax !== "" || filters.yearMin !== "" || filters.yearMax !== "";

  function handleReset() {
    onChange(EMPTY_FILTERS);
    onCommit();
  }

  return (
    <div className="explorer-filters-bar">
      <label className="explorer-filter-field">
        {t("explorer.filterPlayer")}
        <input
          type="text"
          value={filters.playerName}
          placeholder={t("explorer.filterPlayerPlaceholder")}
          onChange={(e) => set("playerName", e.target.value)}
          onKeyDown={handleKeyDown}
        />
      </label>
      <select
        className="explorer-filter-color"
        value={filters.playerWhite ? "white" : "black"}
        onChange={(e) => set("playerWhite", e.target.value === "white")}
        disabled={filters.playerName.trim() === ""}
      >
        <option value="white">{t("common.white")}</option>
        <option value="black">{t("common.black")}</option>
      </select>
      <label className="explorer-filter-field explorer-filter-field--range">
        {t("explorer.filterRating")}
        <span className="explorer-filter-range">
          <input
            type="text"
            inputMode="numeric"
            value={filters.ratingMin}
            placeholder="min"
            onChange={(e) => set("ratingMin", e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <span>–</span>
          <input
            type="text"
            inputMode="numeric"
            value={filters.ratingMax}
            placeholder="max"
            onChange={(e) => set("ratingMax", e.target.value)}
            onKeyDown={handleKeyDown}
          />
        </span>
      </label>
      <label className="explorer-filter-field explorer-filter-field--range">
        {t("explorer.filterYear")}
        <span className="explorer-filter-range">
          <input
            type="text"
            inputMode="numeric"
            value={filters.yearMin}
            placeholder="min"
            onChange={(e) => set("yearMin", e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <span>–</span>
          <input
            type="text"
            inputMode="numeric"
            value={filters.yearMax}
            placeholder="max"
            onChange={(e) => set("yearMax", e.target.value)}
            onKeyDown={handleKeyDown}
          />
        </span>
      </label>
      <button className="explorer-filter-apply" onClick={onCommit}>
        {t("home.search")}
      </button>
      {hasActiveFilter && (
        <button className="explorer-filter-reset" onClick={handleReset} title={t("explorer.filterReset")}>
          ✕
        </button>
      )}
    </div>
  );
}
