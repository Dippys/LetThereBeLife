//! Goal selection: urgency scoring, objective access search (water, food,
//! resources, shelter), and the selected policy action.

use super::exploration::{exploration_target, varied_exploration_heading};
use crate::{
    InventoryView, Material, NeedKind, PhysicalNeedsView, PhysicalPerception, WorldPosition,
    policy::{ExplorationHeading, FoodValues, PhysicalGoal, PolicyReason},
    structures::{SHELTER_WOOD_COST, StructureState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicySelection {
    pub(crate) goal: PhysicalGoal,
    pub(crate) target: Option<WorldPosition>,
    pub(crate) reason: PolicyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyAction {
    pub(crate) goal: PhysicalGoal,
    pub(crate) target: WorldPosition,
    pub(crate) reason: PolicyReason,
    pub(crate) duration: u64,
}

#[cfg(test)]
pub(crate) fn select(
    origin: WorldPosition,
    needs: PhysicalNeedsView,
    inventory: InventoryView,
    perception: &PhysicalPerception,
) -> PolicySelection {
    select_with_exploration(origin, needs, inventory, perception, None).0
}

pub(crate) fn select_with_exploration(
    origin: WorldPosition,
    needs: PhysicalNeedsView,
    inventory: InventoryView,
    perception: &PhysicalPerception,
    exploration_heading: Option<ExplorationHeading>,
) -> (PolicySelection, Option<ExplorationHeading>) {
    let selection = match most_urgent(needs) {
        Some(NeedKind::Thirst) => PolicySelection {
            goal: PhysicalGoal::SeekWater,
            target: nearest_water_access(origin, perception),
            reason: PolicyReason::ThirstThreshold,
        },
        Some(NeedKind::Hunger) if FoodValues::truth().carried(inventory) > 0 => PolicySelection {
            goal: PhysicalGoal::Eat,
            target: Some(origin),
            reason: PolicyReason::HungerThreshold,
        },
        Some(NeedKind::Hunger) => PolicySelection {
            goal: PhysicalGoal::SeekFood,
            target: nearest_resource_access(origin, perception, |kind| {
                FoodValues::truth().is_food(kind) && inventory.can_add(kind)
            }),
            reason: PolicyReason::HungerThreshold,
        },
        Some(NeedKind::Rest) => PolicySelection {
            goal: PhysicalGoal::Sleep,
            target: nearest_shelter_access(origin, perception).or(Some(origin)),
            reason: PolicyReason::RestThreshold,
        },
        Some(NeedKind::Exposure) => shelter_selection(
            origin,
            inventory,
            perception,
            PolicyReason::ExposureThreshold,
        ),
        None => shelter_selection(origin, inventory, perception, PolicyReason::NoUrgentNeed),
    };
    let Some(heading) = exploration_heading else {
        return (selection, None);
    };
    if (selection.target.is_some() && selection.goal != PhysicalGoal::Wait)
        || (selection.goal == PhysicalGoal::Wait && is_safe_anchor(origin, perception))
    {
        return (selection, None);
    }
    let heading = varied_exploration_heading(needs.agent, origin, heading);
    exploration_target(origin, perception, heading).map_or(
        (selection, None),
        |(target, heading)| {
            (
                PolicySelection {
                    goal: PhysicalGoal::Explore,
                    target: Some(target),
                    reason: selection.reason,
                },
                Some(heading),
            )
        },
    )
}

/// The need past its threshold with the highest relative urgency, if any.
pub(super) fn most_urgent(needs: PhysicalNeedsView) -> Option<NeedKind> {
    let most = NeedKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let level = match kind {
                NeedKind::Hunger => needs.hunger,
                NeedKind::Thirst => needs.thirst,
                NeedKind::Rest => needs.rest,
                NeedKind::Exposure => needs.exposure,
            };
            level.threshold_reached.then_some((
                urgency_score(level.value, level.threshold),
                urgency_tie_rank(kind),
                kind,
            ))
        })
        .max_by_key(|&(score, tie, _)| (score, tie))
        .map(|(_, _, kind)| kind);
    // Nobody sleeps or warms up on while dying of thirst: once thirst is past
    // its threshold it comes before cold and tiredness.
    match most {
        Some(NeedKind::Rest | NeedKind::Exposure) if needs.thirst.threshold_reached => {
            Some(NeedKind::Thirst)
        }
        other => other,
    }
}

fn is_safe_anchor(origin: WorldPosition, perception: &PhysicalPerception) -> bool {
    perception
        .drinkable_water
        .iter()
        .any(|water| origin.x.abs_diff(water.position.x) + origin.y.abs_diff(water.position.y) <= 1)
        || nearest_shelter_access(origin, perception) == Some(origin)
}

pub(super) fn shelter_selection(
    origin: WorldPosition,
    inventory: InventoryView,
    perception: &PhysicalPerception,
    reason: PolicyReason,
) -> PolicySelection {
    if let Some(access) = nearest_shelter_access(origin, perception) {
        if reason == PolicyReason::ExposureThreshold {
            return PolicySelection {
                goal: if access == origin {
                    PhysicalGoal::Sleep
                } else {
                    PhysicalGoal::SeekShelter
                },
                target: Some(access),
                reason,
            };
        }
        return nearest_resource_access(origin, perception, |kind| inventory.can_add(kind)).map_or(
            PolicySelection {
                goal: PhysicalGoal::Wait,
                target: Some(origin),
                reason,
            },
            |target| PolicySelection {
                goal: PhysicalGoal::GatherMaterial,
                target: Some(target),
                reason,
            },
        );
    }

    if inventory.amount(Material::Wood) >= SHELTER_WOOD_COST {
        if let Some(site) = nearest_build_site(origin, perception) {
            return PolicySelection {
                goal: PhysicalGoal::BuildShelter,
                target: Some(site),
                reason,
            };
        }
    }

    let needs_wood = inventory.amount(Material::Wood) < SHELTER_WOOD_COST;
    let material = nearest_resource_access(origin, perception, |kind| {
        inventory.can_add(kind) && needs_wood && kind == Material::Wood
    });
    let fallback = (reason == PolicyReason::NoUrgentNeed)
        .then(|| nearest_resource_access(origin, perception, |kind| inventory.can_add(kind)))
        .flatten();
    material.or(fallback).map_or(
        PolicySelection {
            goal: PhysicalGoal::Wait,
            target: Some(origin),
            reason,
        },
        |target| PolicySelection {
            goal: PhysicalGoal::GatherMaterial,
            target: Some(target),
            reason: if material.is_some() {
                PolicyReason::ShelterMaterials
            } else {
                reason
            },
        },
    )
}

pub(super) fn nearest_shelter_access(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<WorldPosition> {
    perception
        .structures
        .iter()
        .filter(|structure| {
            structure.state == StructureState::Complete
                && structure.kind == crate::StructureKind::Shelter
        })
        .flat_map(|structure| cardinal_neighbors(structure.position))
        .filter(|candidate| candidate_available(origin, perception, *candidate))
        .min_by_key(|candidate| target_key(origin, *candidate))
}

pub(super) fn nearest_build_site(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<WorldPosition> {
    // Never build on a shore cell: a structure there can wall off the only way
    // to a small pond's water.
    let shore = |cell: WorldPosition| {
        perception
            .drinkable_water
            .iter()
            .any(|water| cell.x.abs_diff(water.position.x) + cell.y.abs_diff(water.position.y) <= 1)
    };
    // Nor where it would shut anyone in. Cells out of view count as open.
    let open =
        |cell: WorldPosition| !perception.area.contains(cell) || traversable(perception, cell);
    cardinal_neighbors(origin)
        .filter(|candidate| {
            candidate_available(origin, perception, *candidate)
                && !shore(*candidate)
                && !crate::structures::would_enclose(*candidate, open)
                && perception
                    .reserved_cells
                    .binary_search_by_key(&(candidate.y, candidate.x), |cell| (cell.y, cell.x))
                    .is_err()
        })
        .min_by_key(|candidate| (candidate.y, candidate.x))
}

const fn urgency_score(value: u16, threshold: u16) -> u32 {
    (value as u32).saturating_mul(10_000) / threshold as u32
}

const fn urgency_tie_rank(kind: NeedKind) -> u8 {
    match kind {
        NeedKind::Thirst => 3,
        NeedKind::Exposure => 2,
        NeedKind::Hunger => 1,
        NeedKind::Rest => 0,
    }
}

pub(super) fn nearest_water_access(
    origin: WorldPosition,
    perception: &PhysicalPerception,
) -> Option<WorldPosition> {
    perception
        .drinkable_water
        .iter()
        .flat_map(|water| cardinal_neighbors(water.position))
        .filter(|candidate| candidate_available(origin, perception, *candidate))
        .min_by_key(|candidate| target_key(origin, *candidate))
}

pub(super) fn nearest_resource_access(
    origin: WorldPosition,
    perception: &PhysicalPerception,
    accepts: impl Fn(Material) -> bool,
) -> Option<WorldPosition> {
    perception
        .resources
        .iter()
        .filter(|resource| accepts(resource.resource.kind))
        .flat_map(|resource| {
            std::iter::once(resource.position).chain(cardinal_neighbors(resource.position))
        })
        .filter(|candidate| candidate_available(origin, perception, *candidate))
        .min_by_key(|candidate| target_key(origin, *candidate))
}

fn cardinal_neighbors(position: WorldPosition) -> std::array::IntoIter<WorldPosition, 4> {
    [
        WorldPosition {
            x: position.x,
            y: position.y - 1,
        },
        WorldPosition {
            x: position.x - 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x + 1,
            y: position.y,
        },
        WorldPosition {
            x: position.x,
            y: position.y + 1,
        },
    ]
    .into_iter()
}

fn traversable(perception: &PhysicalPerception, position: WorldPosition) -> bool {
    perception
        .reachable_cells
        .binary_search_by_key(&(position.y, position.x), |candidate| {
            (candidate.y, candidate.x)
        })
        .is_ok()
}

pub(super) fn candidate_available(
    origin: WorldPosition,
    perception: &PhysicalPerception,
    position: WorldPosition,
) -> bool {
    traversable(perception, position)
        && (position == origin
            || (!perception
                .agents
                .iter()
                .any(|agent| agent.position == position)
                && perception
                    .claimed_targets
                    .binary_search_by_key(&(position.y, position.x), |target| (target.y, target.x))
                    .is_err()))
}

fn target_key(origin: WorldPosition, target: WorldPosition) -> (u64, i64, i64) {
    (
        origin.x.abs_diff(target.x) + origin.y.abs_diff(target.y),
        target.y,
        target.x,
    )
}
