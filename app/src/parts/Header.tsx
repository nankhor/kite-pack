/** App name, and the PS5 chip, which waits for the Kite helper. */
export function Header() {
  return (
    <header className="header">
      <span className="app-name">Kite Pack</span>
      <button className="ps5-chip" disabled title="Sending to the PS5 comes with the Kite helper">
        <span className="dot" aria-hidden="true" />
        <span className="chip-name">PS5</span>
        <span className="chip-note">Not set up</span>
      </button>
    </header>
  );
}
