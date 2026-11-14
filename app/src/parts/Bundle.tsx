import type { BundleItem, BundlePlan, BundleSelection } from "../types";
import { formatSize } from "../model";

interface Props {
  selection: BundleSelection;
  /** Whether it can all go in, from the packer's own read of the files. */
  plan: BundlePlan | null;
  onAddFolder: () => void;
  onAddPackage: () => void;
  onAddBackport: () => void;
  onRemove: (selection: BundleSelection) => void;
}

function summary(plan: BundlePlan | null): string | null {
  if (!plan || plan.items.length === 0) return null;
  const dlc = plan.items.filter((i) => i.kind === "dlc").length;
  const backport = plan.items.some((i) => i.kind === "backport");
  const parts = [];
  if (dlc > 0) parts.push(`${dlc} ${dlc === 1 ? "add-on" : "add-ons"}`);
  if (backport) parts.push("a backport");
  const what = parts.join(" and ");
  const emulator =
    plan.emulatorFrom === "builtin"
      ? ", with the DLC emulator Kite Pack carries"
      : plan.emulatorFrom === "picked"
        ? ", with the DLC emulator you picked"
        : "";
  return `${what} will be inside the packed copy${emulator}.`;
}

function Chip({ item, onRemove }: { item: BundleItem; onRemove: () => void }) {
  const bad = !!item.problem;
  return (
    <span className={bad ? "bundle-chip bad" : "bundle-chip"} title={item.problem ?? item.path}>
      <span className="bundle-chip-kind">{item.kind === "backport" ? "Backport" : "DLC"}</span>
      {item.name}
      {item.bytes > 0 && <span className="bundle-chip-size">{formatSize(item.bytes)}</span>}
      <button className="bundle-chip-remove" aria-label={`Remove ${item.name}`} onClick={onRemove}>
        ×
      </button>
    </span>
  );
}

/** DLC and a backport to fold into the packed copy, before it is written. */
export function Bundle({ selection, plan, onAddFolder, onAddPackage, onAddBackport, onRemove }: Props) {
  const items = plan?.items ?? [];
  const chosen = items.length > 0;
  const note = plan?.problem ?? summary(plan);

  function remove(path: string) {
    onRemove({
      dlc: selection.dlc.filter((p) => p !== path),
      backport: selection.backport === path ? null : selection.backport,
    });
  }

  return (
    <section className="bundle" aria-label="Inside the packed copy">
      <div className="bundle-head">
        <h2>Inside the packed copy</h2>
        <span className="bundle-hint">Off the PS5 unless you add them here.</span>
      </div>
      <div className="bundle-actions">
        <button className="small-button" onClick={onAddFolder}>
          Add DLC folder
        </button>
        <button className="small-button" onClick={onAddPackage}>
          Add DLC package
        </button>
        <button className="small-button" onClick={onAddBackport}>
          Add backport
        </button>
      </div>
      {chosen && (
        <div className="bundle-chips">
          {items.map((item) => (
            <Chip key={`${item.kind}:${item.path}`} item={item} onRemove={() => remove(item.path)} />
          ))}
        </div>
      )}
      {note && (
        <p className={plan?.problem ? "bundle-note warn" : "bundle-note"} role={plan?.problem ? "alert" : "status"}>
          {note}
        </p>
      )}
      {!chosen && (
        <p className="bundle-note">
          Nothing yet. A dump that already carries its DLC keeps it; anything added here is folded in
          before packing.
        </p>
      )}
    </section>
  );
}
