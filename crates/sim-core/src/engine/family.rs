//! Family life: who grew up together, couples forming and drifting apart.

use crate::cognition::belief_seconds;
use crate::life::{ADULT_AGE, SECONDS_PER_YEAR};
use crate::{AgentActivity, AgentId, CoupleEvent, Engine, PhysicalPerception, Tie};

/// Children younger than this who spend time together grow up feeling like
/// siblings: neither will want the other as a partner.
const CHILDHOOD_AGE: u32 = 10;
/// How well two people must know (familiarity) and trust each other to pair.
const COUPLE_FAMILIARITY: u8 = 100;
const COUPLE_TRUST: u8 = crate::DEFAULT_TRUST;
/// A partnership fades after this long without seeing each other (two years).
const PARTNER_FADE_SECONDS: u32 = 2 * SECONDS_PER_YEAR as u32;
/// Someone who has seen nobody else they could pair with for this long (eight
/// years) stops minding having grown up with someone.
const DESPERATION_SECONDS: u32 = 8 * SECONDS_PER_YEAR as u32;

impl Engine {
    /// Before deciding: notes who it is growing up with, lets a partnership fade
    /// if the partner has been gone too long, and pairs with someone in view
    /// when the feeling is mutual.
    pub(super) fn pair_up(&mut self, agent: AgentId, perception: &PhysicalPerception) {
        if !self.policy_options.social {
            return;
        }
        let now = belief_seconds(self.time);
        let age = self.age_of(agent);
        let others: Vec<AgentId> = perception
            .agents
            .iter()
            .filter(|other| other.id != agent && other.activity != AgentActivity::Dead)
            .map(|other| other.id)
            .collect();
        if age < CHILDHOOD_AGE {
            for &other in &others {
                if self.age_of(other) < CHILDHOOD_AGE {
                    let social = &mut self.minds.get_mut(agent).social;
                    if let Some(slot) = social.slot_of(other) {
                        social.mark_raised_together(slot);
                    }
                }
            }
            return;
        }
        if age < ADULT_AGE {
            return;
        }
        let social = &self.minds.get_mut(agent).social;
        if let Some((slot, _)) = social.partner() {
            if now.saturating_sub(social.last_seen(slot)) > PARTNER_FADE_SECONDS {
                self.minds.get_mut(agent).social.clear_partner(slot);
            }
            return;
        }
        let candidates: Vec<AgentId> = others
            .into_iter()
            .filter(|&other| self.could_pair(agent, other))
            .collect();
        // Someone it grew up with doesn't count as a prospect.
        let social = &self.minds.get_mut(agent).social;
        let prospect = candidates.iter().any(|&other| {
            social
                .slot_of(other)
                .is_none_or(|slot| !social.raised_together(slot))
        });
        if prospect {
            self.minds.get_mut(agent).last_eligible_seen = now;
        }
        let best = candidates
            .into_iter()
            .filter(|&other| self.wants(agent, other) && self.wants(other, agent))
            .max_by_key(|&other| {
                let social = &self.minds.get(agent).expect("agent has a mind").social;
                let slot = social.slot_of(other).expect("wanted means known");
                (
                    u16::from(social.familiarity(slot)) + u16::from(social.trust(slot)),
                    u32::MAX - other.get(),
                )
            });
        let Some(partner) = best else {
            return;
        };
        for (from, to) in [(agent, partner), (partner, agent)] {
            let social = &mut self.minds.get_mut(from).social;
            if let Some(slot) = social.slot_of(to) {
                social.set_tie(slot, Tie::Partner);
            }
        }
        self.couple_events.push(CoupleEvent {
            first: agent,
            second: partner,
            at: self.time,
        });
    }

    /// Whether `other` is someone `agent` could in principle pair with: an
    /// unpartnered adult of the other sex who isn't family.
    fn could_pair(&self, agent: AgentId, other: AgentId) -> bool {
        let (me, them) = (self.life_of(agent), self.life_of(other));
        me.sex != them.sex
            && self.age_of(other) >= ADULT_AGE
            && self
                .minds
                .get(other)
                .is_none_or(|mind| mind.social.partner().is_none())
            && self
                .minds
                .get(agent)
                .and_then(|mind| mind.social.tie_with(other))
                .is_none()
    }

    /// Whether `agent` would pair with `other`: knows and trusts them well
    /// enough, and didn't grow up with them (unless it has long had no one else).
    fn wants(&self, agent: AgentId, other: AgentId) -> bool {
        let Some(mind) = self.minds.get(agent) else {
            return false;
        };
        let Some(slot) = mind.social.slot_of(other) else {
            return false;
        };
        let social = &mind.social;
        let adult_since = self.life_of(agent).born as i64 + i64::from(ADULT_AGE) * SECONDS_PER_YEAR;
        let lonely_since = (mind.last_eligible_seen as i64).max(adult_since);
        let desperate = self.life_seconds() - lonely_since > i64::from(DESPERATION_SECONDS);
        social.familiarity(slot) >= COUPLE_FAMILIARITY
            && social.trust(slot) >= COUPLE_TRUST
            && social.partner().is_none()
            && (!social.raised_together(slot) || desperate)
    }

    /// Couples formed during the latest tick (for logs and tools).
    pub fn couple_events(&self) -> &[CoupleEvent] {
        &self.couple_events
    }

    /// `agent`'s partner, if it has one.
    pub fn partner_of(&self, agent: AgentId) -> Option<AgentId> {
        self.minds
            .get(agent)?
            .social
            .partner()
            .map(|(_, partner)| partner)
    }
}
