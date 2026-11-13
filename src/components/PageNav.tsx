import { Fragment, useState } from "react";
import { useTranslation } from "react-i18next";

interface PageNavProps {
  page: number;
  pageSize: number;
  /** Tailles de page proposees ; le selecteur n'est affiche que si
      onPageSizeChange est fourni. */
  pageSizes?: number[];
  onPageSizeChange?: (size: number) => void;
  total: number;
  loading: boolean;
  onPageChange: (page: number) => void;
  emptyLabel: string;
}

export function PageNav({ page, pageSize, pageSizes, onPageSizeChange, total, loading, onPageChange, emptyLabel }: PageNavProps) {
  const { t } = useTranslation();
  const [rowInput, setRowInput] = useState("");

  function jumpToRow() {
    const row = parseInt(rowInput, 10);
    if (!Number.isFinite(row) || row < 1) return;
    const lastPage = Math.max(0, Math.ceil(total / pageSize) - 1);
    const target = Math.min(Math.max(0, Math.floor((row - 1) / pageSize)), lastPage);
    onPageChange(target);
    setRowInput("");
  }

  return (
    <Fragment>
      {loading ? (
        <span className="games-loading">⏳ {t("home.searching")}</span>
      ) : (
        <span>
          {total === 0
            ? emptyLabel
            : t("home.pageOf", {
                from: page * pageSize + 1,
                to: Math.min((page + 1) * pageSize, total),
                total,
              })}
        </span>
      )}
      <button disabled={loading || page === 0} onClick={() => onPageChange(page - 1)}>
        {t("home.previous")}
      </button>
      <button disabled={loading || (page + 1) * pageSize >= total} onClick={() => onPageChange(page + 1)}>
        {t("home.next")}
      </button>
      {total > pageSize && (
        <span className="page-jump">
          <input
            type="number"
            min={1}
            max={total}
            placeholder={t("home.rowNumber")}
            value={rowInput}
            onChange={(e) => setRowInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") jumpToRow();
            }}
          />
        </span>
      )}
      {onPageSizeChange && pageSizes && (
        <label className="page-size">
          {t("home.rowsPerPage")}
          <select value={pageSize} disabled={loading} onChange={(e) => onPageSizeChange(Number(e.currentTarget.value))}>
            {pageSizes.map((size) => (
              <option key={size} value={size}>
                {size}
              </option>
            ))}
          </select>
        </label>
      )}
    </Fragment>
  );
}
