import { formatSize } from "../model";
import type { GameInfo, InstalledGame, JobView } from "../types";

interface Props {
  game: GameInfo;
  job: JobView | null;
  done: boolean;
  /** This game as the console already has it, when it does. */
  onPs5?: InstalledGame | null;
}

/** The copy being packed: its id and size, and once done what packing saved. */
export function Facts({ game, job, done, onPs5 }: Props) {
  const packed = done ? (job?.packedBytes ?? null) : null;
  // A convert writes the game as it is: there is nothing saved to show.
  const convert = job?.format === "pkg";
  const saved = !convert && packed !== null && game.totalBytes > packed ? game.totalBytes - packed : null;
  return (
    <dl className="facts">
      {game.titleId && (
        <div>
          <dt>Title ID</dt>
          <dd>{game.titleId}</dd>
        </div>
      )}
      {game.totalBytes > 0 && (
        <div>
          <dt>Size</dt>
          <dd>{formatSize(game.totalBytes)}</dd>
        </div>
      )}
      {packed !== null && (
        <div>
          <dt>{convert ? "Package" : "Packed"}</dt>
          <dd>{formatSize(packed)}</dd>
        </div>
      )}
      {saved !== null && (
        <div className="saved">
          <dt>Saved</dt>
          <dd>{formatSize(saved)}</dd>
        </div>
      )}
      {onPs5 && (
        <div className="on-ps5">
          <dt>Already on the PS5</dt>
          <dd>{onPs5.updateInstalled ? "Update installed" : onPs5.version ? `v${onPs5.version}` : "Installed"}</dd>
        </div>
      )}
    </dl>
  );
}
