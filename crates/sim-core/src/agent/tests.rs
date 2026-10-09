//! Layout tests for compact agent records.

use std::mem::{align_of, size_of};

use super::*;
use crate::scheduler::ScheduledEvent;

#[test]
fn foundational_agent_records_remain_compact() {
    assert_eq!(size_of::<AgentId>(), 4);
    assert_eq!(align_of::<AgentId>(), 4);
    assert_eq!(size_of::<CompactPosition>(), 4);
    assert_eq!(size_of::<AgentActivity>(), 1);
    assert_eq!(size_of::<AgentRecord>(), 6);
    assert_eq!(size_of::<RouteState>(), 6);
    assert_eq!(size_of::<Option<RouteState>>(), 8);
    assert_eq!(size_of::<ScheduledEvent>(), 32);
}
