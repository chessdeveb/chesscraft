import { useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { GameReportMoveEval } from "../types";

interface GameReportChartProps {
  moves: GameReportMoveEval[];
  currentNodeId: number;
  onSelect: (nodeId: number) => void;
}

const WIDTH = 600;
const HEIGHT = 90;
const BASELINE = HEIGHT / 2;

function formatEval(m: GameReportMoveEval): string {
  if (m.scoreMate !== null) return `#${m.scoreMate}`;
  const pawns = (m.scoreCp ?? 0) / 100;
  return (pawns > 0 ? "+" : "") + pawns.toFixed(2);
}

/** "12. Nc3" / "12... Nc3" -- prefixe de coup standard, points de
    suspension pour un coup noir. */
function moveLabel(m: GameReportMoveEval): string {
  if (m.ply === 0 || !m.san) return "";
  const moveNumber = Math.ceil(m.ply / 2);
  const isWhite = m.ply % 2 === 1;
  return `${moveNumber}${isWhite ? "." : "..."} ${m.san}`;
}

/** Courbe d'évaluation façon chess.com : ligne orange sur fond clair, aire
    remplie vers la ligne médiane (clair au-dessus = avantage Blancs, gris
    en-dessous = avantage Noirs) -- clic ou survol pour sauter au coup
    correspondant dans la notation. */
export function GameReportChart({ moves, currentNodeId, onSelect }: GameReportChartProps) {
  const { t } = useTranslation();
  const clipIdAbove = `game-report-clip-above-${useId()}`;
  const clipIdBelow = `game-report-clip-below-${useId()}`;
  const points = useMemo(
    () =>
      moves.map((m) => ({
        x: (m.ply / Math.max(1, moves[moves.length - 1].ply)) * WIDTH,
        y: HEIGHT - (m.whiteWinPercent / 100) * HEIGHT,
        move: m,
      })),
    [moves]
  );

  if (points.length < 2) return null;

  const first = points[0];
  const last = points[points.length - 1];
  const areaPath =
    `M ${first.x.toFixed(1)} ${BASELINE} ` +
    points.map((p) => `L ${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(" ") +
    ` L ${last.x.toFixed(1)} ${BASELINE} Z`;
  const linePath = points.map((p, i) => `${i === 0 ? "M" : "L"} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(" ");
  const segmentWidth = WIDTH / (points.length - 1);
  const currentPoint = points.find((p) => p.move.nodeId === currentNodeId);

  return (
    <svg className="game-report-chart" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none">
      <clipPath id={clipIdAbove}>
        <rect x={0} y={0} width={WIDTH} height={BASELINE} />
      </clipPath>
      <clipPath id={clipIdBelow}>
        <rect x={0} y={BASELINE} width={WIDTH} height={HEIGHT - BASELINE} />
      </clipPath>
      <rect x={0} y={0} width={WIDTH} height={HEIGHT} className="game-report-chart-bg" />
      <path d={areaPath} className="game-report-chart-area-above" clipPath={`url(#${clipIdAbove})`} />
      <path d={areaPath} className="game-report-chart-area-below" clipPath={`url(#${clipIdBelow})`} />
      <line x1={0} y1={BASELINE} x2={WIDTH} y2={BASELINE} className="game-report-chart-midline" />
      <path d={linePath} className="game-report-chart-line" />
      {currentPoint && (
        <line
          x1={currentPoint.x}
          y1={0}
          x2={currentPoint.x}
          y2={HEIGHT}
          className="game-report-chart-cursor"
        />
      )}
      {points.map((p) => (
        <rect
          key={`hit-${p.move.nodeId}`}
          x={Math.max(0, p.x - segmentWidth / 2)}
          y={0}
          width={segmentWidth}
          height={HEIGHT}
          className="game-report-chart-hit"
          onClick={() => onSelect(p.move.nodeId)}
        >
          <title>
            {p.move.ply === 0
              ? `${t("gameReport.startPosition")} ${t("gameReport.evalLabel")} : ${formatEval(p.move)}`
              : `${moveLabel(p.move)} ${t("gameReport.evalLabel")} : ${formatEval(p.move)}`}
          </title>
        </rect>
      ))}
    </svg>
  );
}
