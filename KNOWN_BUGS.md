# Known Bugs & Limitations

| ID | Severity | Status | Description |
|---|---|---|---|
| KB-001 | — | **Closed (2026-11-14)** — director confirmed mouse speed feels right on real hardware; the doubling was a test-tool artefact. | In the headless test environment (Xvfb + xdotool), every simulated mouse move arrives as **two identical raw motion events**, so the camera turns twice as far. Game code adds up deltas correctly (unit-tested: 600 counts = 90°). Believed to be a test-environment artefact; please check that turning feels like Minecraft at 100 % sensitivity. |
| KB-002 | Low | Open | On X11, the mouse can only be *confined* to the window (not *locked*), so the hidden cursor can drift to the window edge. Raw mouse input keeps working, so the camera is unaffected. |
| KB-003 | Low | By design (for now) | `Esc` only releases the mouse; there is no pause menu yet (Phase 11). |
| KB-004 | Low | By design (for now) | Editing `settings.toml` while the game runs has no effect until restart; the game also rewrites the file on start (comments you add are not kept). |
| KB-005 | Info | Open | Untested platforms: Windows, macOS. Linux on real hardware confirmed working by the director (2026-11-14). |
| KB-006 | Low | Open (Phase 10) | Each visible section costs 1–2 draw calls (~200 at render distance 8). Fine for now; multi-draw-indirect planned to reduce CPU cost at large render distances. |
| KB-007 | Low | Open (Phase 5) | With Minecraft resource packs, grass-block sides will lack their tinted overlay (`grass_block_side_overlay`) until overlay support is added. Our own textures are unaffected. |
| KB-008 | Info | Fixed (Phase 4) | ~~Temporary test terrain~~ — replaced by the Overworld generator. |
| KB-009 | Low | Open | Leaves in the far distance can look thinner because mipmapped alpha is averaged before the cut-out test (same behaviour as Minecraft). |
| KB-010 | Low | Open | Rivers sometimes form closed loops and tiny ponds appear inland. Tuning planned. |
| KB-011 | Low | Open | Mountain peaks are smoother than Minecraft's jagged peaks. |
| KB-012 | Low | Open | Water is a full block, doesn't flow, and there is no swimming (player sinks). Phases 6/8. |
| KB-013 | Low | Open | Caves are as bright as the surface — no lighting system yet (Phase 8). |
| KB-014 | Low | Open | Ore veins are clipped at chunk borders. |
| KB-015 | Info | Open | Translucent faces inside one section aren't depth-sorted (only sections are). |
| KB-016 | Low | Open | Lore tablets can't be read yet (Phase 5: interaction). |
| KB-017 | Info | Open | Ruins may occasionally intersect caves or cut slightly into hillsides (they sit on the height at their centre). |
