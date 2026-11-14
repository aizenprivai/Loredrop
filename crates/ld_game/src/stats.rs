//! Player health, hunger and air — Minecraft Java's rules.
//!
//! **Health:** 20 points (10 hearts). After being hurt the player is
//! invulnerable for 10 ticks; within the next 10 ticks only damage *greater*
//! than the last hit counts, and only the difference is applied.
//!
//! **Hunger:** food level 0–20 (shanks), hidden *saturation* (starts at 5)
//! and *exhaustion*. Actions add exhaustion (sprinting 0.1 per block,
//! jumping 0.05, sprint-jumping 0.2, mining 0.005, attacking 0.1, taking
//! damage 0.1). Every 4 exhaustion removes 1 saturation, or 1 food if
//! saturation is empty.
//!
//! **Regeneration:** with full food and saturation left, heal fast (every 10
//! ticks); with food ≥ 18, heal 1 every 80 ticks (each costs 6 exhaustion).
//! At food 0 the player starves: 1 damage every 80 ticks — stopping at 10 HP on
//! Easy, at 1 HP on Normal, never on Hard.
//!
//! **Air:** 300 ticks (15 s) of breath under water; then 2 damage per second.
//! Breath returns at 4 per tick on the surface.

/// Game difficulty (affects starvation now, creatures in Phase 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "peaceful" => Difficulty::Peaceful,
            "easy" => Difficulty::Easy,
            "normal" => Difficulty::Normal,
            "hard" => Difficulty::Hard,
            _ => return None,
        })
    }
}

/// What hurt the player (for death messages and armour rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    Fall,
    Lava,
    Fire,
    Drowning,
    Suffocation,
    Cactus,
    Starvation,
    Void,
    /// Hit by a creature or player (Phase 7).
    Attack,
}

impl DamageSource {
    /// Whether armour reduces this damage (as in Minecraft).
    pub fn armor_applies(self) -> bool {
        matches!(self, DamageSource::Lava | DamageSource::Fire | DamageSource::Cactus | DamageSource::Attack)
    }

    /// The death screen message.
    pub fn death_message(self) -> &'static str {
        match self {
            DamageSource::Fall => "You hit the ground too hard",
            DamageSource::Lava => "You tried to swim in lava",
            DamageSource::Fire => "You burned to death",
            DamageSource::Drowning => "You drowned",
            DamageSource::Suffocation => "You suffocated in a wall",
            DamageSource::Cactus => "You were pricked to death",
            DamageSource::Starvation => "You starved to death",
            DamageSource::Void => "You fell out of the world",
            DamageSource::Attack => "You were slain",
        }
    }
}

pub const MAX_HEALTH: f32 = 20.0;
pub const MAX_FOOD: i32 = 20;
pub const MAX_AIR: i32 = 300;

#[derive(Debug, Clone)]
pub struct PlayerStats {
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    /// Ticks of breath left (can go below 0 briefly while drowning).
    pub air: i32,
    /// Ticks remaining of post-hit invulnerability (20 → 0).
    pub invulnerable: u32,
    last_damage: f32,
    /// Ticks the player keeps burning.
    pub fire_ticks: i32,
    regen_timer: u32,
    /// Set when health reaches 0.
    pub death: Option<DamageSource>,
    /// Ticks since the last hurt (for the red flash on screen).
    pub hurt_flash: u32,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            health: MAX_HEALTH,
            food: MAX_FOOD,
            saturation: 5.0,
            exhaustion: 0.0,
            air: MAX_AIR,
            invulnerable: 0,
            last_damage: 0.0,
            fire_ticks: 0,
            regen_timer: 0,
            death: None,
            hurt_flash: 100,
        }
    }
}

impl PlayerStats {
    pub fn is_dead(&self) -> bool {
        self.death.is_some()
    }

    /// Applies damage (already reduced by armour, if any). Returns the damage
    /// actually taken after invulnerability rules.
    pub fn damage(&mut self, amount: f32, source: DamageSource) -> f32 {
        if amount <= 0.0 || self.is_dead() {
            return 0.0;
        }
        let taken = if self.invulnerable > 10 {
            // Recently hurt: only a harder hit counts, and only the difference.
            if amount <= self.last_damage {
                return 0.0;
            }
            let diff = amount - self.last_damage;
            self.last_damage = amount;
            diff
        } else {
            self.last_damage = amount;
            self.invulnerable = 20;
            amount
        };
        self.health = (self.health - taken).max(0.0);
        self.exhaust(0.1);
        self.hurt_flash = 0;
        if self.health <= 0.0 {
            self.death = Some(source);
        }
        taken
    }

    pub fn heal(&mut self, amount: f32) {
        if !self.is_dead() {
            self.health = (self.health + amount).min(MAX_HEALTH);
        }
    }

    pub fn exhaust(&mut self, amount: f32) {
        self.exhaustion = (self.exhaustion + amount).min(40.0);
    }

    /// Eats food worth `food` points with `saturation_modifier` (Minecraft:
    /// saturation gained = food × modifier × 2, capped at the food level).
    pub fn eat(&mut self, food: i32, saturation_modifier: f32) {
        self.food = (self.food + food).min(MAX_FOOD);
        self.saturation = (self.saturation + food as f32 * saturation_modifier * 2.0).min(self.food as f32);
    }

    /// Whether the player is allowed to sprint (needs more than 3 shanks).
    pub fn can_sprint(&self) -> bool {
        self.food > 6
    }

    /// Advances hunger, regeneration, breath and burning by one tick.
    /// `eyes_in_water`: head under water; `in_water`: touching water (puts out fire).
    pub fn tick(&mut self, difficulty: Difficulty, eyes_in_water: bool, in_water: bool) {
        if self.is_dead() {
            return;
        }
        self.invulnerable = self.invulnerable.saturating_sub(1);
        self.hurt_flash = self.hurt_flash.saturating_add(1);

        // Exhaustion → saturation → food.
        while self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else if difficulty != Difficulty::Peaceful {
                self.food = (self.food - 1).max(0);
            }
        }

        // Regeneration / starvation.
        self.regen_timer += 1;
        if difficulty == Difficulty::Peaceful {
            // Peaceful: hunger refills and health regenerates steadily.
            if self.regen_timer >= 20 {
                self.regen_timer = 0;
                self.heal(1.0);
                self.food = (self.food + 1).min(MAX_FOOD);
            }
        } else if self.saturation > 0.0 && self.food >= MAX_FOOD && self.health < MAX_HEALTH {
            if self.regen_timer >= 10 {
                self.regen_timer = 0;
                let s = self.saturation.min(6.0);
                self.heal(s / 6.0);
                self.exhaust(s);
            }
        } else if self.food >= 18 && self.health < MAX_HEALTH {
            if self.regen_timer >= 80 {
                self.regen_timer = 0;
                self.heal(1.0);
                self.exhaust(6.0);
            }
        } else if self.food <= 0 {
            if self.regen_timer >= 80 {
                self.regen_timer = 0;
                let allowed = match difficulty {
                    Difficulty::Easy => self.health > 10.0,
                    Difficulty::Normal => self.health > 1.0,
                    Difficulty::Hard => true,
                    Difficulty::Peaceful => false,
                };
                if allowed {
                    self.damage(1.0, DamageSource::Starvation);
                }
            }
        } else {
            self.regen_timer = 0;
        }

        // Breath.
        if eyes_in_water {
            self.air -= 1;
            if self.air <= -20 {
                self.air = 0;
                self.damage(2.0, DamageSource::Drowning);
            }
        } else {
            self.air = (self.air.max(0) + 4).min(MAX_AIR);
        }

        // Burning: 1 damage per second; water puts it out.
        if in_water {
            self.fire_ticks = 0;
        }
        if self.fire_ticks > 0 {
            if self.fire_ticks % 20 == 0 {
                self.damage(1.0, DamageSource::Fire);
            }
            self.fire_ticks -= 1;
        }
    }
}

/// Minecraft's armour formula: damage after armour with `defense` points
/// and `toughness`. Armour absorbs 4 % per effective point (max 80 %), and
/// strong hits pierce weak armour more.
pub fn armor_reduction(damage: f32, defense: f32, toughness: f32) -> f32 {
    let effective = (defense / 5.0).max(defense - 4.0 * damage / (toughness + 8.0)).min(20.0);
    damage * (1.0 - effective / 25.0)
}

/// Fall damage for a landing after falling `distance` blocks (Minecraft:
/// one point per block beyond 3).
pub fn fall_damage(distance: f64) -> f32 {
    // The tiny epsilon absorbs floating-point drift (a 20-block fall can
    // measure 20.0000001 blocks, which must still be 17 damage, not 18).
    (distance - 3.0 - 1e-6).ceil().max(0.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invulnerability_only_lets_bigger_hits_through() {
        let mut s = PlayerStats::default();
        assert_eq!(s.damage(4.0, DamageSource::Attack), 4.0);
        assert_eq!(s.damage(3.0, DamageSource::Attack), 0.0, "weaker hit during immunity is ignored");
        assert_eq!(s.damage(6.0, DamageSource::Attack), 2.0, "stronger hit only deals the difference");
        assert_eq!(s.health, 14.0);
        for _ in 0..20 {
            s.tick(Difficulty::Normal, false, false);
        }
        assert_eq!(s.damage(1.0, DamageSource::Attack), 1.0, "immunity has worn off");
    }

    #[test]
    fn hunger_drains_through_saturation_first() {
        let mut s = PlayerStats::default();
        s.exhaust(4.0 * 5.0); // burns all 5 saturation
        s.tick(Difficulty::Normal, false, false);
        assert_eq!((s.saturation, s.food), (0.0, 20));
        s.exhaust(8.0);
        s.tick(Difficulty::Normal, false, false);
        assert_eq!(s.food, 18);
        assert!(s.can_sprint());
        s.food = 6;
        assert!(!s.can_sprint(), "3 shanks or less: no sprinting");
    }

    #[test]
    fn regenerates_with_high_food_and_starves_at_zero() {
        let mut s = PlayerStats { health: 10.0, saturation: 0.0, food: 18, ..Default::default() };
        for _ in 0..80 {
            s.tick(Difficulty::Normal, false, false);
        }
        assert_eq!(s.health, 11.0, "food 18: +1 health every 80 ticks");
        let mut starving = PlayerStats { health: 12.0, food: 0, saturation: 0.0, ..Default::default() };
        for _ in 0..80 * 20 {
            starving.tick(Difficulty::Easy, false, false);
        }
        assert_eq!(starving.health, 10.0, "Easy: starvation stops at 10 HP");
        let mut hard = PlayerStats { health: 3.0, food: 0, saturation: 0.0, ..Default::default() };
        for _ in 0..80 * 4 {
            hard.tick(Difficulty::Hard, false, false);
        }
        assert_eq!(hard.death, Some(DamageSource::Starvation));
    }

    #[test]
    fn drowning_after_15_seconds_and_breath_recovers() {
        let mut s = PlayerStats::default();
        for _ in 0..300 {
            s.tick(Difficulty::Normal, true, true);
        }
        assert_eq!(s.health, 20.0, "15 s of breath");
        for _ in 0..20 {
            s.tick(Difficulty::Normal, true, true);
        }
        assert_eq!(s.health, 18.0, "then 2 damage per second");
        for _ in 0..75 {
            s.tick(Difficulty::Normal, false, false);
        }
        assert_eq!(s.air, MAX_AIR);
    }

    #[test]
    fn fire_burns_and_water_extinguishes() {
        let mut s = PlayerStats { fire_ticks: 100, ..Default::default() };
        for _ in 0..41 {
            s.tick(Difficulty::Normal, false, false);
        }
        assert!(s.health < 20.0);
        s.tick(Difficulty::Normal, false, true);
        assert_eq!(s.fire_ticks, 0);
    }

    #[test]
    fn fall_damage_starts_after_three_blocks() {
        assert_eq!(fall_damage(3.0), 0.0);
        assert_eq!(fall_damage(3.4), 1.0);
        assert_eq!(fall_damage(10.0), 7.0);
        assert_eq!(fall_damage(23.0), 20.0, "a 23-block fall is lethal without armour");
        assert_eq!(fall_damage(20.000_000_1), 17.0, "floating-point drift doesn't add a point");
    }

    #[test]
    fn armor_formula_matches_minecraft() {
        // Full diamond (20 points, toughness 8 total): a 10-damage hit → 10 × (1 − 17.5/25) = 3.
        assert!((armor_reduction(10.0, 20.0, 8.0) - 3.0).abs() < 1e-4);
        // Full iron (15 points): 6 damage → 6 × (1 − 12/25) = 3.12.
        assert!((armor_reduction(6.0, 15.0, 0.0) - 3.12).abs() < 1e-4);
        assert_eq!(armor_reduction(5.0, 0.0, 0.0), 5.0);
    }

    #[test]
    fn eating_restores_food_and_saturation() {
        let mut s = PlayerStats { food: 10, saturation: 0.0, ..Default::default() };
        s.eat(4, 0.3); // apple
        assert_eq!(s.food, 14);
        assert!((s.saturation - 2.4).abs() < 1e-5);
    }
}
