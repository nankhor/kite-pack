# Delete as you go

Approved by Nankhor on 2026-11-23.

## Why

A folder pack keeps the whole packed game on the PC until the end, even when it goes to the PS5 and
**Delete the packed folder** is on. A 580 GB game that packs to about 300 GB needs about 300 GB free
on the PC, for a copy nobody keeps.

## Goal

With **Save as: Folder**, **Send to PS5** and **Delete the packed folder** on, and the Kite helper
0.4.0, the PC holds at most about 4 parts (about 16 GB) plus the small loose files at any time,
whatever the game's size. Everything else works as today.

Non-goals: packing straight to the PS5 with no copy on the PC (Forge only writes to FTP/SMB, without
resume, and would read everything back over Wi-Fi); images; resuming a stopped pack.

## Approaches

1. **Shrink parts the PS5 has (picked).** Once the PS5 confirms a part, cut it on the PC to its 64-byte
   header. Forge still finds every file at the end and writes the final headers into them; Kite Pack
   puts each header in place on the PS5 as it does for any part sent early.
2. **Delete the parts.** Forge reopens every part at the end to write its header, and reads the whole
   pack back to check it: both fail on a missing file. Would need the headers kept elsewhere and the
   same changes to the check, for no gain over 1.
3. **Write straight to the PS5 over FTP.** Forge supports it, but one dropped connection loses the whole
   pack, the end check reads everything back over the network one request at a time, and it needs an
   FTP server on the PS5. Rejected.

## Design

**Order of a part's life:** written by Forge, closed, checked (every chunk unpacked and compared with
what went in), announced, sent, confirmed by the PS5, then cut to its header the next time Forge is
about to start a new part. A part is never cut before the PS5 has confirmed it.

**Forge's packer (`third_party/forge-zstd`)**, when Kite Pack has set a volume hook:
- records a CRC-32 of each chunk's raw bytes as it compresses them;
- when a volume closes, reads it back (through the hook) and checks every chunk decodes to its CRC;
- before opening the next volume, calls the hook, which may cut confirmed parts and wait;
- the reader can open a pack where some volumes are cut to their header (only their header is read).

**Forge's core (`third_party/forge-core`)**, when the hook is set:
- the space check counts at most the hook's number of full parts (4) instead of the whole packed size;
- the end check accepts cut volumes (their header must still match the index) and compares the content
  of every file outside them as before. Files with a chunk in a cut volume were checked when their
  volume closed; a check line says how many.

**Pipeline (`kite-pipeline`):** a shared `LetGo` handle goes to the packer in the pack request when the
job sends a folder and deletes the packed folder. The send-ahead worker adds each part the PS5 confirms
to it. While letting go, a PS5 that can't be reached is waited for (the part is tried again), so the
disk can't fill while the PS5 is away; any other failure ends the letting go, and the pack carries on
as it does today. The worker's room check for a copy (Windows) counts parts, not the whole pack.

**Kite Pack's packer (`kite-forge`):** the hook. Before each new volume it cuts every confirmed part not
yet cut, then waits while 2 finished parts are still waiting for the PS5 (until one is confirmed, the
letting go ends, or the job is cancelled).

**Sending the rest (`kite-ps5`):** a part sent early whose copy on the PC is cut to its header still
counts: its header is read from the PC, written on the PS5 and read back, as for any early part.

**Session (`kite-session`):** the packed size counts cut parts at their full size.

**Screen:** under **Delete the packed folder**, when parts go as they are packed: each part leaves the
PC once the PS5 has it, so packing needs about 16 GB free.

## Risks

- A part is unreadable on the PC once cut. If the PS5 copy is lost after that, the game must be packed
  again. Only with **Delete the packed folder** on, where the PC copy is thrown away anyway.
- The per-part check reads each part back once (a few seconds per 4 GB part, usually from the cache).
- Packing pauses whenever the PS5 falls 2 parts behind; on Wi-Fi the total time stays about the same.
