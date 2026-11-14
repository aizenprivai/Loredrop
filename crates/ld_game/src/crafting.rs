//! Crafting and smelting recipes, loaded from `assets/data/recipes.toml`.
//!
//! Matching works like Minecraft's:
//! * **Shaped** recipes compare the grid's filled area (trimmed of empty rows
//!   and columns) against the pattern, so a 2×2 recipe works anywhere in a
//!   3×3 grid. The mirror image of a pattern matches too.
//! * **Shapeless** recipes need exactly their ingredients, in any slots.
//! * Ingredients may be a **tag** (`#planks`) meaning "any of these items".

use crate::inventory::ItemStack;
use crate::items::{ItemId, ItemRegistry};
use serde::Deserialize;
use std::collections::HashMap;

/// A set of acceptable items for one ingredient slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Ingredient(pub Vec<ItemId>);

impl Ingredient {
    pub fn matches(&self, item: ItemId) -> bool {
        self.0.contains(&item)
    }
}

#[derive(Debug, Clone)]
pub struct ShapedRecipe {
    pub width: usize,
    pub height: usize,
    /// Row-major cells; `None` = must be empty.
    pub cells: Vec<Option<Ingredient>>,
    pub result: ItemStack,
}

#[derive(Debug, Clone)]
pub struct ShapelessRecipe {
    pub ingredients: Vec<Ingredient>,
    pub result: ItemStack,
}

#[derive(Debug, Clone)]
pub struct SmeltingRecipe {
    pub input: Ingredient,
    pub result: ItemStack,
    /// Ticks to smelt one item.
    pub time: u32,
}

/// All recipes and fuels.
#[derive(Debug, Clone, Default)]
pub struct RecipeBook {
    pub shaped: Vec<ShapedRecipe>,
    pub shapeless: Vec<ShapelessRecipe>,
    pub smelting: Vec<SmeltingRecipe>,
    /// Burn time in ticks per fuel item.
    pub fuels: HashMap<ItemId, u32>,
}

#[derive(Deserialize)]
struct RecipesFile {
    #[serde(default)]
    tags: HashMap<String, Vec<String>>,
    #[serde(default)]
    shaped: Vec<ShapedEntry>,
    #[serde(default)]
    shapeless: Vec<ShapelessEntry>,
    #[serde(default)]
    smelting: Vec<SmeltingEntry>,
    #[serde(default)]
    fuels: HashMap<String, u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapedEntry {
    result: String,
    count: Option<u16>,
    pattern: Vec<String>,
    key: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapelessEntry {
    result: String,
    count: Option<u16>,
    ingredients: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SmeltingEntry {
    input: String,
    result: String,
    count: Option<u16>,
    time: Option<u32>,
}

impl RecipeBook {
    pub fn from_toml(text: &str, items: &ItemRegistry) -> Result<Self, String> {
        let file: RecipesFile = toml::from_str(text).map_err(|e| format!("recipes file: {e}"))?;
        let item = |name: &str| items.id(name).ok_or_else(|| format!("recipes: unknown item '{name}'"));
        let ingredient = |name: &str| -> Result<Ingredient, String> {
            match name.strip_prefix('#') {
                Some(tag) => {
                    let names = file.tags.get(tag).ok_or_else(|| format!("recipes: unknown tag '#{tag}'"))?;
                    Ok(Ingredient(names.iter().map(|n| item(n)).collect::<Result<_, _>>()?))
                }
                None => Ok(Ingredient(vec![item(name)?])),
            }
        };
        let mut book = RecipeBook::default();
        for e in &file.shaped {
            let height = e.pattern.len();
            let width = e.pattern.iter().map(|r| r.chars().count()).max().unwrap_or(0);
            if !(1..=3).contains(&height) || !(1..=3).contains(&width) {
                return Err(format!("recipe for '{}': pattern must be 1–3 rows of 1–3 characters", e.result));
            }
            let mut cells = Vec::with_capacity(width * height);
            for row in &e.pattern {
                let chars: Vec<char> = row.chars().collect();
                for x in 0..width {
                    match chars.get(x).copied().unwrap_or(' ') {
                        ' ' => cells.push(None),
                        c => {
                            let name = e.key.get(&c.to_string()).ok_or_else(|| {
                                format!("recipe for '{}': '{c}' is not in the key", e.result)
                            })?;
                            cells.push(Some(ingredient(name)?));
                        }
                    }
                }
            }
            book.shaped.push(ShapedRecipe {
                width,
                height,
                cells,
                result: ItemStack::new(item(&e.result)?, e.count.unwrap_or(1)),
            });
        }
        for e in &file.shapeless {
            book.shapeless.push(ShapelessRecipe {
                ingredients: e.ingredients.iter().map(|n| ingredient(n)).collect::<Result<_, _>>()?,
                result: ItemStack::new(item(&e.result)?, e.count.unwrap_or(1)),
            });
        }
        for e in &file.smelting {
            book.smelting.push(SmeltingRecipe {
                input: ingredient(&e.input)?,
                result: ItemStack::new(item(&e.result)?, e.count.unwrap_or(1)),
                time: e.time.unwrap_or(200),
            });
        }
        for (name, &ticks) in &file.fuels {
            book.fuels.insert(item(name)?, ticks);
        }
        Ok(book)
    }

    /// The result of crafting with a `size`×`size` grid (row-major), if any.
    pub fn craft_result(&self, grid: &[Option<ItemStack>], size: usize) -> Option<ItemStack> {
        let items: Vec<Option<ItemId>> = grid.iter().map(|s| s.map(|s| s.item)).collect();
        // Trim to the bounding box of filled cells.
        let filled: Vec<(usize, usize)> =
            (0..size * size).filter(|&i| items[i].is_some()).map(|i| (i % size, i / size)).collect();
        if filled.is_empty() {
            return None;
        }
        let (x0, x1) = (filled.iter().map(|p| p.0).min()?, filled.iter().map(|p| p.0).max()?);
        let (y0, y1) = (filled.iter().map(|p| p.1).min()?, filled.iter().map(|p| p.1).max()?);
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let at = |x: usize, y: usize| items[(y0 + y) * size + x0 + x];

        for r in &self.shaped {
            if r.width != w || r.height != h {
                continue;
            }
            for mirror in [false, true] {
                let ok = (0..h).all(|y| {
                    (0..w).all(|x| {
                        let px = if mirror { w - 1 - x } else { x };
                        match (&r.cells[y * w + px], at(x, y)) {
                            (None, None) => true,
                            (Some(ing), Some(item)) => ing.matches(item),
                            _ => false,
                        }
                    })
                });
                if ok {
                    return Some(r.result);
                }
            }
        }
        let present: Vec<ItemId> = items.iter().flatten().copied().collect();
        'recipes: for r in &self.shapeless {
            if r.ingredients.len() != present.len() {
                continue;
            }
            let mut used = vec![false; present.len()];
            for ing in &r.ingredients {
                match (0..present.len()).find(|&i| !used[i] && ing.matches(present[i])) {
                    Some(i) => used[i] = true,
                    None => continue 'recipes,
                }
            }
            return Some(r.result);
        }
        None
    }

    /// Uses up one item from every filled cell (after taking the result).
    pub fn consume_ingredients(grid: &mut [Option<ItemStack>]) {
        for cell in grid.iter_mut() {
            if let Some(s) = cell {
                s.count -= 1;
                if s.count == 0 {
                    *cell = None;
                }
            }
        }
    }

    /// The smelting recipe for an input item, if any.
    pub fn smelting_for(&self, item: ItemId) -> Option<&SmeltingRecipe> {
        self.smelting.iter().find(|r| r.input.matches(item))
    }

    /// Every recipe whose result is `item` (for the recipe browser).
    pub fn recipes_for(
        &self,
        item: ItemId,
    ) -> (Vec<&ShapedRecipe>, Vec<&ShapelessRecipe>, Vec<&SmeltingRecipe>) {
        (
            self.shaped.iter().filter(|r| r.result.item == item).collect(),
            self.shapeless.iter().filter(|r| r.result.item == item).collect(),
            self.smelting.iter().filter(|r| r.result.item == item).collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::registries;

    fn book() -> (ItemRegistry, RecipeBook) {
        let (_, items) = registries();
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/recipes.toml"))
                .unwrap();
        let book = RecipeBook::from_toml(&text, &items).unwrap();
        (items, book)
    }

    fn grid(items: &ItemRegistry, names: &[&str], size: usize) -> Vec<Option<ItemStack>> {
        assert_eq!(names.len(), size * size);
        names
            .iter()
            .map(|n| if n.is_empty() { None } else { Some(ItemStack::new(items.id(n).unwrap(), 1)) })
            .collect()
    }

    #[test]
    fn logs_to_planks_anywhere_in_the_grid() {
        let (items, book) = book();
        let r = book.craft_result(&grid(&items, &["", "", "", "oak_log"], 2), 2).unwrap();
        assert_eq!((r.item, r.count), (items.id("oak_planks").unwrap(), 4));
    }

    #[test]
    fn pickaxe_needs_the_exact_shape_and_tags_work() {
        let (items, book) = book();
        let g = grid(
            &items,
            &["birch_planks", "oak_planks", "spruce_planks", "", "stick", "", "", "stick", ""],
            3,
        );
        assert_eq!(book.craft_result(&g, 3).unwrap().item, items.id("wooden_pickaxe").unwrap());
        // Planks with a gap in the middle row is not a pickaxe (nor any other recipe).
        let wrong = grid(&items, &["oak_planks", "", "oak_planks", "", "stick", "", "", "stick", ""], 3);
        assert_eq!(book.craft_result(&wrong, 3), None);
    }

    #[test]
    fn mirrored_axe_matches_and_small_recipes_fit_in_a_corner() {
        let (items, book) = book();
        let axe =
            grid(&items, &["", "cobblestone", "cobblestone", "", "stick", "cobblestone", "", "stick", ""], 3);
        assert_eq!(book.craft_result(&axe, 3).unwrap().item, items.id("stone_axe").unwrap());
        let table =
            grid(&items, &["", "", "", "", "oak_planks", "oak_planks", "", "oak_planks", "oak_planks"], 3);
        assert_eq!(book.craft_result(&table, 3).unwrap().item, items.id("crafting_table").unwrap());
    }

    #[test]
    fn consume_takes_one_from_each_cell() {
        let (items, _) = book();
        let mut g = grid(&items, &["oak_log", "", "", ""], 2);
        g[0].as_mut().unwrap().count = 3;
        RecipeBook::consume_ingredients(&mut g);
        assert_eq!(g[0].unwrap().count, 2);
    }

    #[test]
    fn smelting_and_fuel() {
        let (items, book) = book();
        let r = book.smelting_for(items.id("raw_iron").unwrap()).unwrap();
        assert_eq!(r.result.item, items.id("iron_ingot").unwrap());
        assert_eq!(
            book.smelting_for(items.id("spruce_log").unwrap()).unwrap().result.item,
            items.id("charcoal").unwrap()
        );
        assert_eq!(book.fuels[&items.id("coal").unwrap()], 1600);
    }

    #[test]
    fn every_tool_is_craftable() {
        let (items, book) = book();
        for (id, d) in items.iter() {
            if matches!(d.kind, crate::items::ItemKind::Tool(_)) {
                assert!(!book.recipes_for(id).0.is_empty(), "no recipe for {}", d.name);
            }
        }
    }
}
