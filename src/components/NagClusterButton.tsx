import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { NagCluster } from "../nag";

interface NagClusterButtonProps {
  cluster: NagCluster;
  activeCodes: Set<number>;
  onToggle: (code: number) => void;
  /** Le coup courant a-t-il ete joue par les Blancs ? Ne sert qu'aux
      clusters autoSide, pour choisir automatiquement le code Blancs ou
      Noirs sans demander le camp. */
  moverWhite: boolean;
}

const EDGE_MARGIN = 8;

/** Un bouton par "famille" de NAG (qualite du coup, avantage Blancs,
    espace...) qui ouvre une petite liste verticale scopee a cette seule
    famille au clic -- pas de grande liste/recherche unique pour les NAG
    geres par l'appli, voir nag.ts::NAG_CLUSTERS. La liste est portee
    directement dans <body> (voir createPortal ci-dessous) : dockview place
    transform/contain sur ses conteneurs de panneaux (optimisation de
    rendu), ce qui cree un nouveau bloc englobant pour tout descendant en
    position fixe -- sans portail, la liste restait piegee dans les limites
    du panneau Notation au lieu du viewport entier, invisible des qu'elle
    aurait du deborder par-dessus un autre panneau ou le bord de l'ecran. */
export function NagClusterButton({ cluster, activeCodes, onToggle, moverWhite }: NagClusterButtonProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const flyoutRef = useRef<HTMLDivElement | null>(null);
  const adjustedRef = useRef(false);

  useEffect(() => {
    if (!open) return;
    function onClickOutside(e: MouseEvent) {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (flyoutRef.current?.contains(target)) return;
      setOpen(false);
    }
    document.addEventListener("mousedown", onClickOutside);
    return () => document.removeEventListener("mousedown", onClickOutside);
  }, [open]);

  // Position initiale (juste sous le bouton) a l'ouverture.
  useEffect(() => {
    if (!open || !triggerRef.current) {
      setPos(null);
      adjustedRef.current = false;
      return;
    }
    const rect = triggerRef.current.getBoundingClientRect();
    setPos({ top: rect.bottom + 4, left: rect.left });
  }, [open]);

  // Une fois la liste montee (donc sa taille reelle connue), la ramene dans
  // le viewport si elle deborde a droite ou en bas -- une seule passe par
  // ouverture (adjustedRef), pas de boucle de mesure/ajustement continue.
  useLayoutEffect(() => {
    if (!open || !pos || adjustedRef.current || !flyoutRef.current) return;
    adjustedRef.current = true;
    const rect = flyoutRef.current.getBoundingClientRect();
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

  const hasActive = cluster.entries.some((n) => activeCodes.has(n.code));

  // Meme symbole des deux cotes (voir NAG_CLUSTERS) : un seul bouton, le
  // code Blancs/Noirs choisi selon la couleur du coup courant plutot que
  // demande explicitement -- entries est toujours [code Blancs, code Noirs].
  // L'infobulle reste volontairement generique (titre du cluster, "Attaque"
  // plutot que "les Blancs ont l'attaque") : preciser le camp au survol
  // contredirait le principe meme du bouton, qui est de ne jamais avoir a y
  // penser.
  if (cluster.autoSide) {
    const [whiteEntry, blackEntry] = cluster.entries;
    const chosen = moverWhite ? whiteEntry : blackEntry;
    const isActive = activeCodes.has(chosen.code);
    return (
      <button
        type="button"
        className={"nag-button nag-cluster-trigger" + (isActive ? " nag-button--active" : "")}
        title={t(cluster.titleKey)}
        aria-label={t(cluster.titleKey)}
        onClick={() => onToggle(chosen.code)}
      >
        {cluster.symbol}
      </button>
    );
  }

  // Acces direct : un seul choix possible dans ce cluster (voir
  // NAG_CLUSTERS -- beaucoup n'ont plus qu'une entree depuis le retrait des
  // codes sans symbole), pas la peine d'ouvrir une liste pour n'en montrer
  // qu'un seul -- le bouton bascule directement ce code au clic.
  if (cluster.entries.length === 1) {
    const only = cluster.entries[0];
    return (
      <button
        type="button"
        className={"nag-button nag-cluster-trigger" + (hasActive ? " nag-button--active" : "")}
        title={t(only.labelKey)}
        aria-label={t(only.labelKey)}
        onClick={() => onToggle(only.code)}
      >
        {cluster.side && <span className={`nag-cluster-dot nag-cluster-dot--${cluster.side}`} />}
        {cluster.symbol}
      </button>
    );
  }

  return (
    <>
      <button
        type="button"
        ref={triggerRef}
        className={"nag-button nag-cluster-trigger" + (hasActive ? " nag-button--active" : "")}
        title={t(cluster.titleKey)}
        aria-label={t(cluster.titleKey)}
        onClick={() => setOpen((v) => !v)}
      >
        {cluster.side && <span className={`nag-cluster-dot nag-cluster-dot--${cluster.side}`} />}
        {cluster.symbol}
      </button>
      {open &&
        pos &&
        createPortal(
          <div className="nag-cluster-flyout" ref={flyoutRef} style={{ top: pos.top, left: pos.left }}>
            {cluster.entries.map((n) => (
              <button
                type="button"
                key={n.code}
                className={"nag-cluster-item" + (activeCodes.has(n.code) ? " nag-cluster-item--active" : "")}
                onClick={() => {
                  onToggle(n.code);
                  setOpen(false);
                }}
              >
                <span className="nag-cluster-item-symbol">{n.symbol}</span>
                <span className="nag-cluster-item-label">{t(n.labelKey)}</span>
              </button>
            ))}
          </div>,
          document.body
        )}
    </>
  );
}
