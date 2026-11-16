//! Furnaces: smelt an input item using fuel, with Minecraft's timings
//! (200 ticks = 10 s per item; coal burns 1600 ticks = 8 items).
//!
//! The world remembers up to [`MAX_FURNACES`] furnaces (their slots and
//! progress); each keeps working while the game runs, wherever it is.

use crate::items::*;

/// How many furnaces the world can hold.
pub const MAX_FURNACES: usize = 8;

/// One placed furnace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Furnace {
    /// Whether this entry is in use.
    pub used: bool,
    /// Block position.
    pub pos: [i32; 3],
    /// Item being smelted.
    pub input: Stack,
    /// Fuel waiting to burn.
    pub fuel: Stack,
    /// Finished items.
    pub output: Stack,
    /// Ticks of fuel left.
    pub burn_left: u16,
    /// Burn time of the current fuel item (for the flame gauge).
    pub burn_total: u16,
    /// Progress on the current item, in ticks.
    pub cook: u16,
    /// Ticks the current item needs.
    pub cook_total: u16,
}

impl Furnace {
    /// An unused entry.
    pub const EMPTY: Furnace = Furnace {
        used: false,
        pos: [0; 3],
        input: Stack::EMPTY,
        fuel: Stack::EMPTY,
        output: Stack::EMPTY,
        burn_left: 0,
        burn_total: 0,
        cook: 0,
        cook_total: 200,
    };

    /// Whether the current input can be smelted into the output slot.
    fn can_smelt(&self) -> Option<(Item, u16)> {
        if self.input.is_empty() {
            return None;
        }
        let (result, time) = smelt_result(self.input.item)?;
        let room = self.output.is_empty()
            || (self.output.item == result && self.output.count < item_def(result).max_stack);
        room.then_some((result, time))
    }

    /// Whether it is burning.
    pub fn lit(&self) -> bool {
        self.burn_left > 0
    }

    /// Advances one tick. Returns true if it started or stopped burning
    /// (so the block can switch between furnace and lit furnace).
    pub fn tick(&mut self) -> bool {
        let was_lit = self.lit();
        let smelt = self.can_smelt();
        if self.burn_left == 0 && smelt.is_some() && !self.fuel.is_empty() {
            let t = fuel_ticks(self.fuel.item);
            if t > 0 {
                self.burn_left = t;
                self.burn_total = t;
                self.fuel.take(1);
            }
        }
        if self.burn_left > 0 {
            self.burn_left -= 1;
            if let Some((result, time)) = smelt {
                self.cook_total = time;
                self.cook += 1;
                if self.cook >= time {
                    self.cook = 0;
                    self.input.take(1);
                    if self.output.is_empty() {
                        self.output = Stack::new(result, 1);
                    } else {
                        self.output.count += 1;
                    }
                }
            } else {
                self.cook = 0;
            }
        } else {
            // Unlit: progress slowly falls back, as on PC.
            self.cook = self.cook.saturating_sub(2);
        }
        was_lit != self.lit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::*;

    #[test]
    fn coal_smelts_eight_items() {
        let mut f = Furnace {
            used: true,
            input: Stack::new(RAW_IRON, 10),
            fuel: Stack::new(COAL, 1),
            ..Furnace::EMPTY
        };
        assert!(f.tick(), "lights up");
        for _ in 0..1700 {
            f.tick();
        }
        assert_eq!(f.output, Stack::new(IRON_INGOT, 8));
        assert_eq!(f.input.count, 2);
        assert!(!f.lit());
    }

    #[test]
    fn no_fuel_wasted_without_input() {
        let mut f = Furnace { used: true, fuel: Stack::new(OAK_PLANKS as Item, 3), ..Furnace::EMPTY };
        for _ in 0..100 {
            f.tick();
        }
        assert_eq!(f.fuel.count, 3);
        f.input = Stack::new(SAND as Item, 1);
        for _ in 0..201 {
            f.tick();
        }
        assert_eq!(f.output, Stack::new(GLASS as Item, 1));
    }
}
