# Loredrop — Roadmap

Each phase ends with a report and requires director approval before the next begins.

| Phase | Name | Status |
|---|---|---|
| 0 | Game design & interview | ✅ Complete (GDD approved) |
| 1 | Technical research & architecture | ✅ Complete (architecture approved) |
| 2 | Core engine foundation | ✅ Complete (approved; real-hardware test pending) |
| 3 | Voxel engine & chunk system | ✅ Complete (approved) |
| 4 | Procedural world generation | ✅ Complete (approved) |
| 5 | Core gameplay | ✅ Complete (approved) |
| 6 | Survival systems | ✅ Complete (approved) |
| 7 | Entities & AI | ✅ Complete (approved) |
| 8 | World simulation & atmosphere (incl. seasons) | 🟡 In progress |
| 9 | Persistence & advanced systems (incl. mod runtime investigation) | ⏳ |
| 10 | Optimisation & performance (incl. LOD) | ⏳ |
| 11 | Polish & release candidate | ⏳ |
| Later | Fire realm, final realm & boss, ruins/lore expansion, WASM/native mods, resource-pack sounds, multiplayer, mobile | ⏳ |

### NumWorks edition (side project, `numworks/`)

A 3D Loredrop for NumWorks calculators, started 2026-11-16 at the director's
request. Separate engine, shared data. Same rule: each phase ends with a
report and needs approval.

| Phase | Name | Status |
|---|---|---|
| NW1 | 3D engine, world generation, exploration & building | ✅ Done on PC — waiting for the director's real-calculator test |
| NW2 | Survival: health, hunger, fall damage, inventory, crafting, tools, furnace | ✅ Done on PC — waiting for report approval and a real-calculator test |
| NW3 | Creatures (animals, zombies, skeletons, spiders, the Hollow), combat, armour, dropped items, ruins with lore | ✅ Done on PC — waiting for report approval and a real-calculator test |
| NW4 | Speed & polish: automatic quality, timing display, torches + light, plants, smarter creatures | ✅ Done on PC — waiting for report approval; real-calculator tuning still to come |

Notes:
- The settings system (with descriptions for every option) and the performance
  overlay start in Phase 2 and grow with every phase.
- Ruins with lore start as simple structures in Phase 4; the lore journal comes later.
