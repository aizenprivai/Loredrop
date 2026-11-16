//! Health, hunger, saturation, breath and damage, with Minecraft's rules
//! (Normal difficulty):
//!
//! * 20 health points (10 hearts) and 20 food points.
//! * Actions add *exhaustion* (sprinting 0.1 per block, jumping 0.05,
//!   sprint-jumping 0.2, swimming 0.01 per block, mining 0.005 per block,
//!   taking damage 0.1). Every 4 exhaustion uses 1 saturation, or 1 food
//!   point when saturation is empty.
//! * With full food and some saturation, health comes back fast (every 10
//!   ticks); with 18+ food, 1 point every 4 s. At 0 food you lose 1 point
//!   every 4 s, down to 1 (Normal difficulty never starves you to death).
//! * Falling more than 3 blocks hurts 1 per extra block. Breath lasts 15 s
//!   under water, then drowning deals 2 every second. Lava deals 4 every
//!   half second and sets you on fire (1 per second for 15 s, water puts it
//!   out). After being hurt you can't be hurt again for half a second.

/// Breath when not under water, in ticks (15 s).
pub const MAX_AIR: i16 = 300;
/// Most health (10 hearts).
pub const MAX_HEALTH: f32 = 20.0;
/// Most food (10 drumsticks).
pub const MAX_FOOD: u8 = 20;
/// Ticks after being hurt during which further damage is ignored.
const HURT_COOLDOWN: u8 = 10;
/// Ticks on fire after leaving lava.
const FIRE_TICKS: u16 = 300;

/// Why the player got hurt (for the status line and death message).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    /// Fell too far.
    Fall,
    /// Ran out of breath under water.
    Drowning,
    /// Touched lava.
    Lava,
    /// Burning.
    Fire,
    /// Starving.
    Starvation,
}

impl Cause {
    /// Death message.
    pub fn death_message(self) -> &'static str {
        match self {
            Cause::Fall => "You hit the ground too hard.",
            Cause::Drowning => "You drowned.",
            Cause::Lava => "You tried to swim in lava.",
            Cause::Fire => "You burned to death.",
            Cause::Starvation => "You starved to death.",
        }
    }
}

/// What the player did this tick (filled in by the game from the player's motion).
#[derive(Clone, Copy, Default, Debug)]
pub struct Activity {
    /// Blocks walked while sprinting.
    pub sprint_dist: f32,
    /// Blocks moved while swimming.
    pub swim_dist: f32,
    /// Jumped this tick.
    pub jumped: bool,
    /// The jump was a sprint-jump.
    pub sprinting: bool,
    /// Eyes under water.
    pub eyes_in_water: bool,
    /// Body in water.
    pub in_water: bool,
    /// Body in lava.
    pub in_lava: bool,
    /// Fall distance if the player landed this tick (0 otherwise).
    pub landed_fall: f32,
}

/// The player's health, food and breath.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vitals {
    /// Health points (0–20; 2 per heart).
    pub health: f32,
    /// Food points (0–20).
    pub food: u8,
    /// Hidden food buffer used before food points (0–food).
    pub saturation: f32,
    /// Accumulated effort; every 4 costs saturation or food.
    pub exhaustion: f32,
    /// Breath left in ticks (`MAX_AIR` when not under water).
    pub air: i16,
    /// Ticks on fire left.
    pub fire: u16,
    food_timer: u16,
    hurt_cooldown: u8,
    /// Ticks since last hurt (for the red flash), saturating.
    pub since_hurt: u16,
    /// What last hurt the player.
    pub last_cause: Option<Cause>,
}

impl Default for Vitals {
    fn default() -> Self {
        Self::new()
    }
}

impl Vitals {
    /// Full health and food, as at spawn.
    pub const fn new() -> Self {
        Self {
            health: MAX_HEALTH,
            food: MAX_FOOD,
            saturation: 5.0,
            exhaustion: 0.0,
            air: MAX_AIR,
            fire: 0,
            food_timer: 0,
            hurt_cooldown: 0,
            since_hurt: 1000,
            last_cause: None,
        }
    }

    /// Whether the player is dead.
    pub fn dead(&self) -> bool {
        self.health <= 0.0
    }

    /// Whether there's enough food to sprint (more than 3 drumsticks).
    pub fn can_sprint(&self) -> bool {
        self.food > 6
    }

    /// Adds exhaustion (e.g. 0.005 for mining a block).
    pub fn exhaust(&mut self, x: f32) {
        self.exhaustion = (self.exhaustion + x).min(40.0);
    }

    /// Hurts the player unless just hurt. Returns true if damage was dealt.
    pub fn hurt(&mut self, amount: f32, cause: Cause) -> bool {
        if self.hurt_cooldown > 0 || amount <= 0.0 || self.dead() {
            return false;
        }
        self.health = (self.health - amount).max(0.0);
        self.hurt_cooldown = HURT_COOLDOWN;
        self.since_hurt = 0;
        self.last_cause = Some(cause);
        self.exhaust(0.1);
        true
    }

    fn heal(&mut self, amount: f32) {
        self.health = (self.health + amount).min(MAX_HEALTH);
    }

    /// Eats food worth `food` points with saturation modifier `saturation`.
    pub fn eat(&mut self, food: u8, saturation: f32) {
        self.food = (self.food + food).min(MAX_FOOD);
        self.saturation = (self.saturation + food as f32 * saturation * 2.0).min(self.food as f32);
    }

    /// Advances one tick.
    pub fn tick(&mut self, a: &Activity) {
        self.hurt_cooldown = self.hurt_cooldown.saturating_sub(1);
        self.since_hurt = self.since_hurt.saturating_add(1);

        // Falling.
        if a.landed_fall > 3.0 && !a.in_water {
            let dmg = libm::ceilf(a.landed_fall - 3.0);
            self.hurt(dmg, Cause::Fall);
        }

        // Breath.
        if a.eyes_in_water {
            self.air -= 1;
            if self.air <= -20 {
                self.air = 0;
                self.hurt(2.0, Cause::Drowning);
            }
        } else {
            self.air = (self.air.max(0) + 4).min(MAX_AIR);
        }

        // Lava and fire.
        if a.in_lava {
            self.fire = FIRE_TICKS;
            self.hurt(4.0, Cause::Lava);
        } else if self.fire > 0 {
            if a.in_water {
                self.fire = 0;
            } else {
                self.fire -= 1;
                if self.fire.is_multiple_of(20) {
                    self.hurt(1.0, Cause::Fire);
                }
            }
        }

        // Effort.
        self.exhaust(a.sprint_dist * 0.1 + a.swim_dist * 0.01);
        if a.jumped {
            self.exhaust(if a.sprinting { 0.2 } else { 0.05 });
        }
        if self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = self.food.saturating_sub(1);
            }
        }

        // Healing and starving.
        self.food_timer += 1;
        if self.saturation > 0.0 && self.food >= MAX_FOOD && self.health < MAX_HEALTH {
            if self.food_timer >= 10 {
                let s = self.saturation.min(6.0);
                self.heal(s / 6.0);
                self.exhaust(s);
                self.food_timer = 0;
            }
        } else if self.food >= 18 && self.health < MAX_HEALTH {
            if self.food_timer >= 80 {
                self.heal(1.0);
                self.exhaust(6.0);
                self.food_timer = 0;
            }
        } else if self.food == 0 {
            if self.food_timer >= 80 {
                if self.health > 1.0 {
                    self.hurt_cooldown = 0;
                    self.hurt(1.0, Cause::Starvation);
                }
                self.food_timer = 0;
            }
        } else {
            self.food_timer = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fall_damage() {
        let mut v = Vitals::new();
        v.tick(&Activity { landed_fall: 3.0, ..Default::default() });
        assert_eq!(v.health, 20.0);
        v.tick(&Activity { landed_fall: 8.4, ..Default::default() });
        assert_eq!(v.health, 14.0, "8.4 blocks → 6 damage");
        let mut v = Vitals::new();
        v.tick(&Activity { landed_fall: 30.0, in_water: true, ..Default::default() });
        assert_eq!(v.health, 20.0, "water breaks the fall");
    }

    #[test]
    fn drowning_after_15_seconds() {
        let mut v = Vitals::new();
        let under = Activity { eyes_in_water: true, in_water: true, ..Default::default() };
        for _ in 0..315 {
            v.tick(&under);
        }
        assert_eq!(v.health, 20.0);
        for _ in 0..10 {
            v.tick(&under);
        }
        assert!(v.health < 20.0);
        assert_eq!(v.last_cause, Some(Cause::Drowning));
    }

    #[test]
    fn hunger_and_regeneration() {
        let mut v = Vitals::new();
        // Sprinting ~400 blocks = 40 exhaustion = 10 points (5 saturation,
        // then 5 food). A few extra blocks make up for float rounding.
        for _ in 0..405 {
            v.tick(&Activity { sprint_dist: 1.0, ..Default::default() });
        }
        assert_eq!(v.saturation, 0.0);
        assert_eq!(v.food, 15);
        // Starving stops at 1 health on Normal.
        v.food = 0;
        for _ in 0..80 * 30 {
            v.tick(&Activity::default());
        }
        assert_eq!(v.health, 1.0);
        // Eating then resting heals.
        v.eat(20, 0.6);
        for _ in 0..20 * 60 {
            v.tick(&Activity::default());
        }
        assert!(v.health > 10.0, "health {}", v.health);
    }

    #[test]
    fn lava_burns() {
        // Food below 18 so natural healing doesn't blur the numbers.
        let mut v = Vitals { food: 17, saturation: 0.0, ..Vitals::new() };
        for _ in 0..10 {
            v.tick(&Activity { in_lava: true, ..Default::default() });
        }
        assert_eq!(v.health, 16.0, "4 damage per half second");
        assert!(v.fire > 0);
        v.tick(&Activity { in_water: true, ..Default::default() });
        assert_eq!(v.fire, 0);
    }
}
