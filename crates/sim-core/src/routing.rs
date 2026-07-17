use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    error::Error,
    fmt,
};

use crate::{
    AgentId, TraversalKind, World, WorldPosition, WorldQueryError, WorldRect,
    agent::CompactPosition,
    placements::SpawnedObjects,
    structures::{StructureId, StructureStore},
    world::MIN_TRAVERSAL_COST,
};

pub const MAX_ROUTE_EXPANSIONS: u16 = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRequest {
    pub destination: WorldPosition,
    pub max_expansions: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteRequestError {
    PolicyControlled,
    MissingAgent,
    DeadAgent,
    AlreadyAtDestination,
    ZeroBudget,
    BudgetTooLarge { requested: u16, maximum: u16 },
    OutsideWorld,
    OutsideActiveArea,
    Unloaded,
    Blocked(TraversalKind),
    Occupied(AgentId),
    BlockedByStructure(StructureId),
    NoPath { expansions: u16 },
    BudgetExhausted { expansions: u16 },
    TimeOverflow,
    RescheduleLimit,
    EventSequenceExhausted,
}

impl fmt::Display for RouteRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "route request failed: {self:?}")
    }
}

impl Error for RouteRequestError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoutePlan {
    pub(crate) next: WorldPosition,
    pub(crate) expansions: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RouteEnvironment<'a> {
    pub(crate) world: &'a World,
    pub(crate) spawned_objects: &'a SpawnedObjects,
    pub(crate) structures: &'a StructureStore,
    pub(crate) active_area: WorldRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RouteNode {
    position: CompactPosition,
    cost: u32,
    parent: u16,
    closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenNode {
    estimated_cost: u32,
    position: CompactPosition,
    node: u16,
}

impl Ord for OpenNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimated_cost
            .cmp(&self.estimated_cost)
            .then_with(|| other.position.y.cmp(&self.position.y))
            .then_with(|| other.position.x.cmp(&self.position.x))
            .then_with(|| other.node.cmp(&self.node))
    }
}

impl PartialOrd for OpenNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Default)]
pub(crate) struct RoutePlanner {
    nodes: Vec<RouteNode>,
    by_position: HashMap<CompactPosition, u16>,
    open: BinaryHeap<OpenNode>,
}

impl RoutePlanner {
    pub(crate) fn plan(
        &mut self,
        environment: RouteEnvironment<'_>,
        origin: WorldPosition,
        request: RouteRequest,
    ) -> Result<RoutePlan, RouteRequestError> {
        validate_request(environment, origin, request)?;
        self.nodes.clear();
        self.by_position.clear();
        self.open.clear();

        let origin = CompactPosition::checked(origin).ok_or(RouteRequestError::OutsideWorld)?;
        let destination =
            CompactPosition::checked(request.destination).ok_or(RouteRequestError::OutsideWorld)?;
        self.nodes.push(RouteNode {
            position: origin,
            cost: 0,
            parent: 0,
            closed: false,
        });
        self.by_position.insert(origin, 0);
        self.open.push(OpenNode {
            estimated_cost: route_heuristic(origin, destination),
            position: origin,
            node: 0,
        });

        let mut expansions = 0_u16;
        while let Some(open) = self.open.pop() {
            let index = usize::from(open.node);
            if self.nodes[index].closed {
                continue;
            }
            self.nodes[index].closed = true;
            expansions += 1;
            if open.position == destination {
                return Ok(RoutePlan {
                    next: self.first_step(open.node).world(),
                    expansions,
                });
            }
            let current = open.position.world();
            for (dx, dy) in [(0_i64, -1_i64), (-1, 0), (1, 0), (0, 1)] {
                let neighbor = WorldPosition {
                    x: current.x + dx,
                    y: current.y + dy,
                };
                if !environment.active_area.contains(neighbor)
                    || environment.structures.structure_at(neighbor).is_some()
                {
                    continue;
                }
                let step = environment
                    .spawned_objects
                    .traversal_step(environment.world, current, neighbor)
                    .map_err(map_query_error)?;
                let Some(step_cost) = step.cost() else {
                    continue;
                };
                let Some(cost) = self.nodes[index].cost.checked_add(u32::from(step_cost)) else {
                    continue;
                };
                let compact =
                    CompactPosition::checked(neighbor).ok_or(RouteRequestError::OutsideWorld)?;
                if let Some(&raw) = self.by_position.get(&compact) {
                    let existing = &mut self.nodes[usize::from(raw)];
                    if cost < existing.cost && !existing.closed {
                        existing.cost = cost;
                        existing.parent = open.node;
                        self.open.push(OpenNode {
                            estimated_cost: cost
                                .saturating_add(route_heuristic(compact, destination)),
                            position: compact,
                            node: raw,
                        });
                    }
                    continue;
                }
                let raw = u16::try_from(self.nodes.len())
                    .map_err(|_| RouteRequestError::BudgetExhausted { expansions })?;
                self.nodes.push(RouteNode {
                    position: compact,
                    cost,
                    parent: open.node,
                    closed: false,
                });
                self.by_position.insert(compact, raw);
                self.open.push(OpenNode {
                    estimated_cost: cost.saturating_add(route_heuristic(compact, destination)),
                    position: compact,
                    node: raw,
                });
            }
            if expansions == request.max_expansions {
                while self.open.peek().is_some_and(|entry| {
                    let node = self.nodes[usize::from(entry.node)];
                    node.closed
                }) {
                    self.open.pop();
                }
                return if self.open.is_empty() {
                    Err(RouteRequestError::NoPath { expansions })
                } else {
                    Err(RouteRequestError::BudgetExhausted { expansions })
                };
            }
        }
        Err(RouteRequestError::NoPath { expansions })
    }

    fn first_step(&self, mut node: u16) -> CompactPosition {
        while self.nodes[usize::from(node)].parent != 0 {
            node = self.nodes[usize::from(node)].parent;
        }
        self.nodes[usize::from(node)].position
    }

    #[cfg(test)]
    pub(crate) fn capacities(&self) -> (usize, usize, usize) {
        (
            self.nodes.capacity(),
            self.by_position.capacity(),
            self.open.capacity(),
        )
    }
}

fn route_heuristic(from: CompactPosition, destination: CompactPosition) -> u32 {
    let dx = i32::from(from.x).abs_diff(i32::from(destination.x));
    let dy = i32::from(from.y).abs_diff(i32::from(destination.y));
    (dx + dy).saturating_mul(u32::from(MIN_TRAVERSAL_COST))
}

fn validate_request(
    environment: RouteEnvironment<'_>,
    origin: WorldPosition,
    request: RouteRequest,
) -> Result<(), RouteRequestError> {
    if request.max_expansions == 0 {
        return Err(RouteRequestError::ZeroBudget);
    }
    if request.max_expansions > MAX_ROUTE_EXPANSIONS {
        return Err(RouteRequestError::BudgetTooLarge {
            requested: request.max_expansions,
            maximum: MAX_ROUTE_EXPANSIONS,
        });
    }
    if request.destination == origin {
        return Err(RouteRequestError::AlreadyAtDestination);
    }
    if !crate::WORLD_GENERATION_BOUNDS.contains(request.destination) {
        return Err(RouteRequestError::OutsideWorld);
    }
    if !environment.active_area.contains(request.destination) {
        return Err(RouteRequestError::OutsideActiveArea);
    }
    if let Some(structure) = environment.structures.structure_at(request.destination) {
        return Err(RouteRequestError::BlockedByStructure(structure));
    }
    match environment
        .spawned_objects
        .standability_at(environment.world, request.destination)
    {
        Ok(crate::Standability::Standable) => Ok(()),
        Ok(crate::Standability::BlockedByWater) => {
            Err(RouteRequestError::Blocked(TraversalKind::BlockedByWater))
        }
        Ok(crate::Standability::BlockedByFeature) => {
            Err(RouteRequestError::Blocked(TraversalKind::BlockedByFeature))
        }
        Err(error) => Err(map_query_error(error)),
    }
}

fn map_query_error(error: WorldQueryError) -> RouteRequestError {
    match error {
        WorldQueryError::OutsideWorldBounds => RouteRequestError::OutsideWorld,
        WorldQueryError::Unloaded => RouteRequestError::Unloaded,
        WorldQueryError::NonCardinalStep => unreachable!("route planner emits cardinal steps"),
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn route_scratch_records_have_fixed_compact_layouts() {
        assert_eq!(size_of::<RouteNode>(), 12);
        assert_eq!(align_of::<RouteNode>(), 4);
        assert_eq!(size_of::<OpenNode>(), 12);
        assert_eq!(align_of::<OpenNode>(), 4);
    }
}
