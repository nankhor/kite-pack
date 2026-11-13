interface Props {
  message: string | null;
  onChooseFolder: () => void;
  onChooseFile: () => void;
}

/** Before a game: one place to drop it, or pick it. */
export function Empty({ message, onChooseFolder, onChooseFile }: Props) {
  return (
    <section className="empty" aria-label="Choose a game">
      <div className="drop-target">
        <h1 className="empty-title">Drop a game here</h1>
        <p className="empty-hint">A dump folder, an exFAT, .ffpkg, .ffpfs or fake .pkg image, or a .zip, .7z or .rar archive.</p>
        <div className="buttons">
          <button className="button primary" onClick={onChooseFolder}>
            Choose folder
          </button>
          <button className="button secondary" onClick={onChooseFile}>
            Choose file
          </button>
        </div>
        {message && (
          <p role="alert" className="message">
            {message}
          </p>
        )}
      </div>
    </section>
  );
}
