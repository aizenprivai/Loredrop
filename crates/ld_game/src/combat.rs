//! Combat rules — Minecraft's modern (1.9+) combat, as chosen in the GDD.
//!
//! **Attack cooldown.** Every weapon has an attack speed (hits per second at
//! full strength). After a swing, attack strength recharges from 0 to 1 over
//! `20 / attack_speed` ticks: a sword (1.6) needs 12.5 ticks, an axe (0.8–1.0)
//! 20–25 ticks, a bare hand (4.0) 5 ticks. Damage scales with
//! `0.2 + strength² × 0.8`, so spam-clicking deals only 20 %.
//!
//! **Critical hits.** Falling, not on the ground, not in water, not
//! sprinting, with strength above 0.9: ×1.5 damage.
//!
//! **Sweep attack.** A fully charged sword hit while standing on the ground
//! and not sprinting also hits nearby enemies for 1 damage (used in Phase 7).
//!
//! Creatures to hit arrive in Phase 7; this module provides the rules and the
//! numbers they will use.

use crate::inventory::ItemStack;
use crate::items::{ItemKind, ItemRegistry, ToolKind};

/// Bare-hand attack: 1 damage, 4 hits per second.
pub const FIST_DAMAGE: f32 = 1.0;
pub const FIST_SPEED: f32 = 4.0;

/// Attack damage and speed of whatever is held.
pub fn weapon_stats(held: Option<ItemStack>, items: &ItemRegistry) -> (f32, f32) {
    match held.map(|s| &items.get(s.item).kind) {
        Some(ItemKind::Tool(t)) => (t.attack_damage, t.attack_speed),
        _ => (FIST_DAMAGE, FIST_SPEED),
    }
}

/// Attack strength (0..1) after `ticks_since_swing` with a weapon of `attack_speed`.
pub fn attack_strength(ticks_since_swing: u32, attack_speed: f32) -> f32 {
    let period = 20.0 / attack_speed;
    ((ticks_since_swing as f32 + 0.5) / period).clamp(0.0, 1.0)
}

/// Everything about the player's situation that affects a hit.
#[derive(Debug, Clone, Copy, Default)]
pub struct AttackContext {
    pub strength: f32,
    pub falling: bool,
    pub on_ground: bool,
    pub in_water: bool,
    pub sprinting: bool,
}

/// The result of one attack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub damage: f32,
    pub critical: bool,
    /// Whether nearby enemies are also hit (sword sweep).
    pub sweep: bool,
}

/// Computes a hit from base weapon damage and the situation.
pub fn compute_hit(base_damage: f32, weapon: Option<ToolKind>, ctx: AttackContext) -> Hit {
    let mut damage = base_damage * (0.2 + ctx.strength * ctx.strength * 0.8);
    let full = ctx.strength > 0.9;
    let critical = full && ctx.falling && !ctx.on_ground && !ctx.in_water && !ctx.sprinting;
    if critical {
        damage *= 1.5;
    }
    let sweep = full && weapon == Some(ToolKind::Sword) && ctx.on_ground && !ctx.sprinting && !critical;
    Hit { damage, critical, sweep }
}

/// Durability a weapon loses per hit: swords 1, other tools 2 (Minecraft).
pub fn weapon_wear(weapon: Option<ToolKind>) -> u16 {
    match weapon {
        Some(ToolKind::Sword) => 1,
        Some(_) => 2,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldown_recharges_at_weapon_speed() {
        assert!(attack_strength(0, 1.6) < 0.1, "right after a swing");
        assert_eq!(attack_strength(12, 1.6), 1.0, "a sword recharges in 12.5 ticks");
        assert!(attack_strength(12, 0.8) < 0.7, "an axe is slower");
        assert_eq!(attack_strength(5, FIST_SPEED), 1.0);
    }

    #[test]
    fn spam_clicking_deals_20_percent_and_crits_multiply() {
        let ctx = AttackContext { strength: 0.0, on_ground: true, ..Default::default() };
        assert!((compute_hit(6.0, Some(ToolKind::Sword), ctx).damage - 1.2).abs() < 1e-5);
        let crit = AttackContext { strength: 1.0, falling: true, ..Default::default() };
        let h = compute_hit(6.0, Some(ToolKind::Sword), crit);
        assert!(h.critical && (h.damage - 9.0).abs() < 1e-5 && !h.sweep);
        let ground = AttackContext { strength: 1.0, on_ground: true, ..Default::default() };
        assert!(compute_hit(6.0, Some(ToolKind::Sword), ground).sweep);
        assert!(!compute_hit(9.0, Some(ToolKind::Axe), ground).sweep, "only swords sweep");
        let sprint = AttackContext { sprinting: true, ..crit };
        assert!(!compute_hit(6.0, Some(ToolKind::Sword), sprint).critical, "no crits while sprinting");
    }
}
