interface Props {
  message: string | null;
  onChooseFolder: () => void;
  onChooseFile: () => void;
  /** Look at the PS5 instead of opening something on this computer. */
  onSeeLibrary?: () => void;
  /** Install asks for a package; Convert for a dump; Pack for a game in any form. */
  kind?: "game" | "package" | "dump";
}

const WORDS = {
  game: ["Choose a game", "Drop a game here", "A dump folder, an exFAT, .ffpkg, .ffpfs or fake .pkg image, or a .zip, .7z or .rar archive."],
  package: [
    "Choose a package",
    "Drop a .pkg or .fpkg here",
    "A fake package for the PS5 to install. It streams from this computer: nothing is copied to the PS5 first.",
  ],
  dump: [
    "Choose a dump",
    "Drop a dump here",
    "A game folder, or an exFAT, .ffpkg or .ffpfs image, to turn into a fake package (.fpkg) the PS5 installs.",
  ],
} as const;

/** Before a game: one place to drop it, or pick it. */
export function Empty({ message, onChooseFolder, onChooseFile, onSeeLibrary, kind = "game" }: Props) {
  const pkg = kind === "package";
  const [label, title, hint] = WORDS[kind];
  return (
    <section className="empty" aria-label={label}>
      <div className="drop-target">
        <h1 className="empty-title">{title}</h1>
        <p className="empty-hint">{hint}</p>
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
