import type { BaseInfo } from "../types";

interface BasesTableProps {
  bases: BaseInfo[];
  selectedPath?: string | null;
  onSelect: (base: BaseInfo) => void;
}

export function BasesTable({ bases, selectedPath, onSelect }: BasesTableProps) {
  if (bases.length === 0) {
    return <p className="notation-empty">Aucune base pour l'instant</p>;
  }

  return (
    <table className="explorer-table">
      <thead>
        <tr>
          <th>Nom</th>
          <th>Parties</th>
          <th>Emplacement</th>
        </tr>
      </thead>
      <tbody>
        {bases.map((b) => (
          <tr
            key={b.path}
            className={selectedPath === b.path ? "explorer-table-row--selected" : ""}
            onClick={() => onSelect(b)}
          >
            <td>
              {b.isReference && <span title="Base de référence">📚 </span>}
              {b.missing && <span title="Fichier introuvable">⚠️ </span>}
              {b.name}
            </td>
            <td>{b.games}</td>
            <td className="bases-table-path">{b.path}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
