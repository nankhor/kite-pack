interface Props {
  message: string | null;
  onChooseFolder: () => void;
  onChooseFile: () => void;
  /** Look at the PS5 instead of opening something on this computer. */
  onSeeLibrary?: () => void;
  /** Install asks for a package; Pack for a game in any form. */
  kind?: "game" | "package";
}

/** Before a game: one place to drop it, or pick it. */
export function Empty({ message, onChooseFolder, onChooseFile, onSeeLibrary, kind = "game" }: Props) {
  const pkg = kind === "package";
  return (
    <section className="empty" aria-label={pkg ? "Choose a package" : "Choose a game"}>
      <div className="drop-target">
        <h1 className="empty-title">{pkg ? "Drop a .pkg or .fpkg here" : "Drop a game here"}</h1>
        <p className="empty-hint">
          {pkg
            ? "A fake package for the PS5 to install. It streams from this computer: nothing is copied to the PS5 first."
            : "A dump folder, an exFAT, .ffpkg, .ffpfs or fake .pkg image, or a .zip, .7z or .rar archive."}
        </p>
        <div className="buttons">
          {pkg ? (
            <button className="button primary" onClick={onChooseFile}>
              Choose a package
            </button>
          ) : (
            <>
              <button className="button primary" onClick={onChooseFolder}>
                Choose folder
              </button>
              <button className="button secondary" onClick={onChooseFile}>
                Choose file
              </button>
            </>
          )}
        </div>
        {onSeeLibrary && (
          <button className="link-button" onClick={onSeeLibrary}>
            See what&rsquo;s on the PS5
          </button>
        )}
        {message && (
          <p role="alert" className="message">
            {message}
          </p>
        )}
      </div>
    </section>
  );
}
