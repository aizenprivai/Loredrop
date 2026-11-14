# Known Bugs & Limitations

| ID | Severity | Status | Description |
|---|---|---|---|
| KB-001 | Medium | **Needs verification on real hardware** | In the headless test environment (Xvfb + xdotool), every simulated mouse move arrives as **two identical raw motion events**, so the camera turns twice as far. Game code adds up deltas correctly (unit-tested: 600 counts = 90°). Believed to be a test-environment artefact; please check that turning feels like Minecraft at 100 % sensitivity. |
| KB-002 | Low | Open | On X11, the mouse can only be *confined* to the window (not *locked*), so the hidden cursor can drift to the window edge. Raw mouse input keeps working, so the camera is unaffected. |
| KB-003 | Low | By design (for now) | `Esc` only releases the mouse; there is no pause menu yet (Phase 11). |
| KB-004 | Low | By design (for now) | Editing `settings.toml` while the game runs has no effect until restart; the game also rewrites the file on start (comments you add are not kept). |
| KB-005 | Info | Open | Untested platforms: Windows, macOS, real GPUs (only Linux + software Vulkan/OpenGL tested). |
| KB-006 | Low | Open (Phase 10) | Each visible section costs 1–2 draw calls (~200 at render distance 8). Fine for now; multi-draw-indirect planned to reduce CPU cost at large render distances. |
| KB-007 | Low | Open (Phase 5) | With Minecraft resource packs, grass-block sides will lack their tinted overlay (`grass_block_side_overlay`) until overlay support is added. Our own textures are unaffected. |
| KB-008 | Info | By design (Phase 3) | Terrain is a temporary test generator (hills + trees, no caves/water/biomes); trees never cross chunk borders. Real generation is Phase 4. |
| KB-009 | Low | Open | Leaves in the far distance can look thinner because mipmapped alpha is averaged before the cut-out test (same behaviour as Minecraft). |
