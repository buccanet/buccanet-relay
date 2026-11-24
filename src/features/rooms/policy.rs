//! Pure room rules, tested without a database.

use buccanet_protocol::api::{DeviceKind, RoomPolicy};
use buccanet_protocol::events::MessageKind;

pub const MAX_MEMBERS_LIMIT: u32 = 1000;
pub const AGENT_BURST_LIMIT: u32 = 10_000;

pub fn validate_policy(policy: &RoomPolicy) -> Result<(), String> {
    if !(2..=MAX_MEMBERS_LIMIT).contains(&policy.max_members) {
        return Err(format!("maxMembers must be between 2 and {MAX_MEMBERS_LIMIT}."));
    }
    if !(1..=AGENT_BURST_LIMIT).contains(&policy.agent_burst) {
        return Err(format!("agentBurst must be between 1 and {AGENT_BURST_LIMIT}."));
    }
    Ok(())
}

/// One commit per epoch: anything framed for another epoch is stale.
pub fn epoch_matches(room_epoch: u64, message_epoch: u64) -> bool {
    room_epoch == message_epoch
}

pub fn has_room_for_one_more(member_count: usize, policy: &RoomPolicy) -> bool {
    member_count < policy.max_members as usize
}

#[derive(Debug, PartialEq, Eq)]
pub enum Burst {
    /// Accept, and store this as the new count of agent messages in a row.
    Accept(u32),
    Exceeded,
}

/// Agents may exchange `agent_burst` application messages in a row; a human message resets the count.
pub fn agent_burst(sender: DeviceKind, kind: MessageKind, run: u32, policy: &RoomPolicy) -> Burst {
    match (sender, kind) {
        (_, MessageKind::Commit) => Burst::Accept(run),
        (DeviceKind::Human, MessageKind::Application) => Burst::Accept(0),
        (DeviceKind::Agent, MessageKind::Application) if run >= policy.agent_burst => Burst::Exceeded,
        (DeviceKind::Agent, MessageKind::Application) => Burst::Accept(run + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RoomPolicy {
        RoomPolicy { approval: true, max_members: 3, agent_burst: 2 }
    }

    #[test]
    fn agents_stop_after_the_burst_until_a_human_speaks() {
        let p = policy();
        assert_eq!(agent_burst(DeviceKind::Agent, MessageKind::Application, 0, &p), Burst::Accept(1));
        assert_eq!(agent_burst(DeviceKind::Agent, MessageKind::Application, 1, &p), Burst::Accept(2));
        assert_eq!(agent_burst(DeviceKind::Agent, MessageKind::Application, 2, &p), Burst::Exceeded);
        assert_eq!(agent_burst(DeviceKind::Agent, MessageKind::Commit, 2, &p), Burst::Accept(2));
        assert_eq!(agent_burst(DeviceKind::Human, MessageKind::Application, 2, &p), Burst::Accept(0));
    }

    #[test]
    fn capacity_and_epochs() {
        let p = policy();
        assert!(has_room_for_one_more(2, &p));
        assert!(!has_room_for_one_more(3, &p));
        assert!(epoch_matches(4, 4));
        assert!(!epoch_matches(5, 4));
    }

    #[test]
    fn policy_bounds() {
        assert!(validate_policy(&policy()).is_ok());
        assert!(validate_policy(&RoomPolicy { max_members: 1, ..policy() }).is_err());
        assert!(validate_policy(&RoomPolicy { agent_burst: 0, ..policy() }).is_err());
    }
}
