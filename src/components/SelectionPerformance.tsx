import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

interface Performance {
  player: string;
  games: number;
  selected: number;
  points: number;
  averageOpponent: number;
  performance: number;
}

interface SelectionPerformanceProps {
  basePath: string;
  selectedIds: Set<number>;
}

export function SelectionPerformance({ basePath, selectedIds }: SelectionPerformanceProps) {
  const { t } = useTranslation();
  const [perf, setPerf] = useState<Performance | null>(null);

  useEffect(() => {
    if (selectedIds.size === 0) {
      setPerf(null);
      return;
    }
    let cancelled = false;
    invoke<Performance | null>("games_performance", { path: basePath, ids: Array.from(selectedIds) })
      .then((result) => {
        if (!cancelled) setPerf(result);
      })
      .catch(() => {
        if (!cancelled) setPerf(null);
      });
    return () => {
      cancelled = true;
    };
  }, [basePath, selectedIds]);

  if (selectedIds.size === 0) return null;
  if (!perf) return <p className="notation-empty">{t("gamesTable.performanceNone")}</p>;
  return (
    <p className="selection-performance">
      {t("gamesTable.performanceSummary", {
        player: perf.player,
        performance: perf.performance,
        points: perf.points,
        games: perf.games,
        selected: perf.selected,
        average: perf.averageOpponent,
      })}
    </p>
  );
}
