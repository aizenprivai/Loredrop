//! Furnace logic (smelting), following Minecraft's rules:
//!
//! * A furnace needs an input with a smelting recipe, room in the output,
//!   and burning fuel.
//! * When the flame goes out and there's something to smelt, one fuel item
//!   is consumed and burns for its burn time (coal: 1600 ticks = 8 items).
//! * Each item takes 200 ticks (10 s). If the fuel runs out mid-way, the
//!   progress cools down by 2 per tick instead of resetting instantly.

use crate::crafting::RecipeBook;
use crate::inventory::ItemStack;
use crate::items::ItemRegistry;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Furnace {
    pub input: Option<ItemStack>,
    pub fuel: Option<ItemStack>,
    pub output: Option<ItemStack>,
    /// Ticks of burn time left / total for the current fuel item.
    pub burn_left: u32,
    pub burn_total: u32,
    /// Smelting progress of the current item / ticks needed.
    pub cook: u32,
    pub cook_total: u32,
}

impl Furnace {
    pub fn is_lit(&self) -> bool {
        self.burn_left > 0
    }

    /// The result of smelting the current input, if it can go into the output.
    fn smeltable(&self, book: &RecipeBook, items: &ItemRegistry) -> Option<(ItemStack, u32)> {
        let input = self.input?;
        let recipe = book.smelting_for(input.item)?;
        let fits = match self.output {
            None => true,
            Some(o) => {
                o.stacks_with(&recipe.result) && o.count + recipe.result.count <= items.get(o.item).max_stack
            }
        };
        fits.then_some((recipe.result, recipe.time))
    }

    /// Advances one tick. Returns true if the lit/unlit state changed (so the
    /// block's appearance should switch).
    pub fn tick(&mut self, book: &RecipeBook, items: &ItemRegistry) -> bool {
        let was_lit = self.is_lit();
        let smelt = self.smeltable(book, items);
        // Light new fuel when needed.
        if self.burn_left == 0
            && smelt.is_some()
            && let Some(fuel) = self.fuel
            && let Some(&ticks) = book.fuels.get(&fuel.item)
        {
            self.burn_left = ticks;
            self.burn_total = ticks;
            let f = self.fuel.as_mut().expect("checked above");
            f.count -= 1;
            if f.count == 0 {
                self.fuel = None;
            }
        }
        if self.burn_left > 0 {
            self.burn_left -= 1;
            match smelt {
                Some((result, time)) => {
                    self.cook_total = time;
                    self.cook += 1;
                    if self.cook >= time {
                        self.cook = 0;
                        match &mut self.output {
                            Some(o) => o.count += result.count,
                            None => self.output = Some(result),
                        }
                        let i = self.input.as_mut().expect("smeltable implies input");
                        i.count -= 1;
                        if i.count == 0 {
                            self.input = None;
                        }
                    }
                }
                None => self.cook = 0,
            }
        } else {
            self.cook = self.cook.saturating_sub(2); // cooling down
        }
        was_lit != self.is_lit()
    }

    /// Everything inside (dropped when the furnace is broken).
    pub fn contents(&self) -> Vec<ItemStack> {
        [self.input, self.fuel, self.output].into_iter().flatten().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::registries;

    fn book(items: &ItemRegistry) -> RecipeBook {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/recipes.toml"))
                .unwrap();
        RecipeBook::from_toml(&text, items).unwrap()
    }

    #[test]
    fn coal_smelts_eight_iron() {
        let (_, items) = registries();
        let book = book(&items);
        let mut f = Furnace {
            input: Some(ItemStack::new(items.id("raw_iron").unwrap(), 10)),
            fuel: Some(ItemStack::new(items.id("coal").unwrap(), 1)),
            ..Default::default()
        };
        assert!(f.tick(&book, &items), "lights up on the first tick");
        for _ in 1..1600 {
            f.tick(&book, &items);
        }
        assert!(!f.is_lit(), "one coal burns for 1600 ticks");
        let out = f.output.unwrap();
        assert_eq!((out.item, out.count), (items.id("iron_ingot").unwrap(), 8));
        assert_eq!(f.input.unwrap().count, 2);
    }

    #[test]
    fn no_fuel_wasted_without_input_and_progress_cools_down() {
        let (_, items) = registries();
        let book = book(&items);
        let mut f =
            Furnace { fuel: Some(ItemStack::new(items.id("coal").unwrap(), 3)), ..Default::default() };
        for _ in 0..50 {
            f.tick(&book, &items);
        }
        assert_eq!(f.fuel.unwrap().count, 3, "nothing to smelt: fuel untouched");
        // A stick burns 100 ticks: not enough for a full 200-tick item.
        let mut f = Furnace {
            input: Some(ItemStack::new(items.id("sand").unwrap(), 1)),
            fuel: Some(ItemStack::new(items.id("stick").unwrap(), 1)),
            ..Default::default()
        };
        for _ in 0..100 {
            f.tick(&book, &items);
        }
        assert_eq!(f.cook, 100);
        for _ in 0..10 {
            f.tick(&book, &items);
        }
        assert_eq!(f.cook, 80, "cools by 2 per tick once the fire is out");
        assert!(f.output.is_none());
    }
}
