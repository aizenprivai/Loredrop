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
| KB-012 | Low | Open (Phase 8) | Water is a full block and doesn't flow. (Swimming was added in Phase 6.) |
| KB-013 | — | Fixed (Phase 8) | ~~Caves are as bright as the surface~~ — lighting implemented. |
| KB-014 | Low | Open | Ore veins are clipped at chunk borders. |
| KB-015 | Info | Open | Translucent faces inside one section aren't depth-sorted (only sections are). |
| KB-016 | — | Fixed (Phase 5) | ~~Lore tablets can't be read~~ — right-click to read; journal on J. |
| KB-017 | Info | Open | Ruins may occasionally intersect caves or cut slightly into hillsides (they sit on the height at their centre). |
| KB-018 | Low | Open | Sand and gravel don't fall when unsupported (falling blocks not implemented yet). |
| KB-019 | Low | Open | If the inventory is full when a menu closes, items left on the cursor or in the crafting grid are lost instead of dropping. |
| KB-020 | Low | Open | A tablet's story era is recognised from the block under it; a player-moved tablet may show a different era's fragment. |
| KB-021 | Info | Open | Simulated input in the test environment doubles mouse-wheel events (like mouse motion); real hardware is unaffected. |
| KB-022 | — | Fixed (Phase 7) | ~~Attacks have nothing to hit~~ — creatures can now be attacked. |
| KB-023 | — | Fixed (Phase 7) | ~~Apples are the only food~~ — animals drop meat, which can be cooked. |
| KB-024 | — | Fixed (Phase 8) | ~~Hostiles only spawn under a roof~~ — they now spawn by light level: caves, and the surface at night. |
| KB-025 | — | Fixed (Phase 7) | ~~The Hollow has no behaviour~~ — anger, block moving and natural spawning implemented. |
| KB-026 | Low | Open (Phase 9) | Creatures are not saved: any creature in an area that unloads (or when the game closes) is removed. The list of Hollow-placed blocks is not saved either, so after a restart breaking them angers nobody. |
| KB-027 | Info | By design | Hollows leave one-block holes in the ground where they pick blocks up (like their Minecraft counterpart). They never take blocks from under the player or another creature, or blocks a Hollow placed. |
| KB-028 | Low | Open | The Hollow's long legs swing as far as a zombie's, so its stride looks exaggerated. |
| KB-030 | Info | By design | Light is computed in boxes reaching 15 blocks around the sections being rebuilt; light that would leave the box and come back in is missed. Only possible more than ~15 blocks from its source, so effects are tiny. |
| KB-031 | Low | Open | Torches can only stand on the ground (no wall torches yet) and look like a thin cross, not a 3D stick. |
| KB-029 | Info | By design (director, 2026-11-14) | Attacking one Hollow angers only that Hollow; breaking a Hollow-placed block angers all Hollows within 16 blocks. |
