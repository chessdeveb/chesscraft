import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

interface GameTypeButtonProps {
  onStartChess960: (positionNumber: number) => void;
}

const EDGE_MARGIN = 8;

/** Bouton "type de jeu" du panneau Echiquier (transport, a cote de la
    navigation) : ouvre un petit formulaire pour demarrer une nouvelle partie
    Chess960 dans cet onglet, a partir d'un numero de position (0-959, schema
    de numerotation standard -- voir chess960_start_fen cote backend). Portail
    dans <body> pour les memes raisons que NagClusterButton : echappe au
    transform/contain de dockview qui pieogerait sinon le popover dans les
    limites du panneau. */
export function GameTypeButton({ onStartChess960 }: GameTypeButtonProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const adjustedRef = useRef(false);

  useEffect(() => {
    if (!open) return;
    function onClickOutside(e: MouseEvent) {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (popoverRef.current?.contains(target)) return;
      setOpen(false);
    }
    document.addEventListener("mousedown", onClickOutside);
    return () => document.removeEventListener("mousedown", onClickOutside);
  }, [open]);

  useEffect(() => {
    if (!open || !triggerRef.current) {
      setPos(null);
      adjustedRef.current = false;
      return;
    }
    const rect = triggerRef.current.getBoundingClientRect();
    setPos({ top: rect.bottom + 4, left: rect.left });
    setValue("");
    // le popover s'ouvre juste au-dessus/dessous du bouton : le focus sur le
    // champ ne peut se faire qu'une fois le champ effectivement monte.
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  useLayoutEffect(() => {
    if (!open || !pos || adjustedRef.current || !popoverRef.current) return;
    adjustedRef.current = true;
    const rect = popoverRef.current.getBoundingClientRect();
    let { top, left } = pos;
    if (rect.right > window.innerWidth - EDGE_MARGIN) {
      left = Math.max(EDGE_MARGIN, window.innerWidth - rect.width - EDGE_MARGIN);
    }
    if (rect.bottom > window.innerHeight - EDGE_MARGIN) {
      const triggerRect = triggerRef.current?.getBoundingClientRect();
      top = triggerRect ? triggerRect.top - rect.height - 4 : Math.max(EDGE_MARGIN, window.innerHeight - rect.height - EDGE_MARGIN);
    }
    if (top !== pos.top || left !== pos.left) setPos({ top, left });
  }, [open, pos]);

  const parsed = Number.parseInt(value, 10);
  const isValid = value.trim() !== "" && Number.isInteger(parsed) && parsed >= 0 && parsed <= 959;

  function start() {
    if (!isValid) return;
    onStartChess960(parsed);
    setOpen(false);
  }

  return (
    <>
      <button
        type="button"
        ref={triggerRef}
        className="game-type-button"
        title={t("board.chess960Button")}
        aria-label={t("board.chess960Button")}
        onClick={() => setOpen((v) => !v)}
      >
        960
      </button>
      {open &&
        pos &&
        createPortal(
          <div className="game-type-popover" ref={popoverRef} style={{ top: pos.top, left: pos.left }}>
            <label className="game-type-popover-label" htmlFor="chess960-position-number">
              {t("board.chess960PositionLabel")}
            </label>
            <div className="game-type-popover-row">
              <input
                id="chess960-position-number"
                ref={inputRef}
                type="number"
                min={0}
                max={959}
                value={value}
                onChange={(e) => setValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") start();
                  if (e.key === "Escape") setOpen(false);
                }}
              />
              <button type="button" onClick={start} disabled={!isValid}>
                {t("board.chess960Start")}
              </button>
            </div>
          </div>,
          document.body
        )}
    </>
  );
}
