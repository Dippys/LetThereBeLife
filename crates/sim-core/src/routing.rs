use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    error::Error,
    fmt,
};

use crate::{
    AgentId, TraversalKind, World, WorldPosition, WorldQueryError, WorldRect,
    agent::CompactPosition, spatial::SpatialIndex,
};

pub const MAX_ROUTE_EXPANSIONS: u16 = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRequest {
    pub destination: WorldPosition,
    pub max_expansions: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteRequestError {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RouteNode {
    position: CompactPosition,
    cost: u32,
    parent: u16,
    closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenNode {
    cost: u32,
    position: CompactPosition,
    node: u16,
}

impl Ord for OpenNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
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
        world: &World,
        occupancy: &SpatialIndex,
        active_area: WorldRect,
        agent: AgentId,
        origin: WorldPosition,
        request: RouteRequest,
    ) -> Result<RoutePlan, RouteRequestError> {
        validate_request(world, occupancy, active_area, agent, origin, request)?;
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
            cost: 0,
            position: origin,
            node: 0,
        });

        let mut expansions = 0_u16;
        while let Some(open) = self.open.pop() {
            let index = usize::from(open.node);
            if self.nodes[index].closed || self.nodes[index].cost != open.cost {
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
                if !active_area.contains(neighbor)
                    || occupancy
                        .occupant(neighbor)
                        .is_some_and(|occupant| occupant != agent)
                {
                    continue;
                }
                let step = world
                    .traversal_step(current, neighbor)
                    .map_err(map_query_error)?;
                let Some(step_cost) = step.cost() else {
                    continue;
                };
                let Some(cost) = open.cost.checked_add(u32::from(step_cost)) else {
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
                            cost,
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
                    cost,
                    position: compact,
                    node: raw,
                });
            }
            if expansions == request.max_expansions {
                while self.open.peek().is_some_and(|entry| {
                    let node = self.nodes[usize::from(entry.node)];
                    node.closed || node.cost != entry.cost
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

fn validate_request(
    world: &World,
    occupancy: &SpatialIndex,
    active_area: WorldRect,
    agent: AgentId,
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
    if !active_area.contains(request.destination) {
        return Err(RouteRequestError::OutsideActiveArea);
    }
    if let Some(occupant) = occupancy.occupant(request.destination)
        && occupant != agent
    {
        return Err(RouteRequestError::Occupied(occupant));
    }
    match world.standability_at(request.destination) {
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
