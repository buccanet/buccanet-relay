//! Who administers a room: an admin sends the commit that replaces the MLS admin list, with
//! the same list for the relay, which decides from it who approves, removes and revokes.

use buccanet_protocol::api::{Accepted, SetAdmins};

use crate::error::{AppError, AppResult};
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::commits::{check_commit, deliver_commit, locked_admin_room, store_commit};
use crate::features::rooms::repo::RoomDoc;
use crate::state::AppState;

pub async fn set(state: &AppState, room_id: &str, device: &DeviceDoc, input: SetAdmins) -> AppResult<Accepted> {
    let (_guard, room) = locked_admin_room(state, room_id, device).await?;
    check_admins(&room, &input.admins)?;
    check_commit(&room, &input.commit)?;
    let accepted = store_commit(state, &room, &device.id, format!("admins:{}", room.epoch), input.commit.clone()).await?;
    state.rooms.set_admins(room_id, &input.admins).await?;
    deliver_commit(state, &room, device, &accepted, input.commit, &room.member_ids_except(&device.id)).await?;
    Ok(accepted)
}

/// An admin list names members of the room, each once, and at least one.
fn check_admins(room: &RoomDoc, admins: &[String]) -> AppResult<()> {
    if admins.is_empty() {
        return Err(AppError::invalid("A room keeps at least one admin."));
    }
    if let Some(outsider) = admins.iter().find(|admin| room.member(admin).is_none()) {
        return Err(AppError::invalid(format!("{outsider} is not a member of this room.")));
    }
    if admins.iter().enumerate().any(|(index, admin)| admins[..index].contains(admin)) {
        return Err(AppError::invalid("The admin list names a member twice."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use mongodb::bson::DateTime;

    use super::*;
    use crate::features::rooms::repo::MemberDoc;

    fn room() -> RoomDoc {
        let member = |id: &str| MemberDoc { device_id: id.into(), admin: id == "ada", joined_at: DateTime::now(), sealed: 0 };
        RoomDoc {
            id: "r".into(),
            group_id: vec![1],
            policy: Default::default(),
            epoch: 3,
            last_seq: 9,
            agent_run: 0,
            members: vec![member("ada"), member("bob")],
            created_at: DateTime::now(),
        }
    }

    #[test]
    fn admin_lists_name_members_once() {
        let list = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert!(check_admins(&room(), &list(&["ada", "bob"])).is_ok());
        assert!(check_admins(&room(), &list(&["bob"])).is_ok());
        assert!(check_admins(&room(), &list(&[])).is_err());
        assert!(check_admins(&room(), &list(&["ada", "zed"])).is_err());
        assert!(check_admins(&room(), &list(&["bob", "bob"])).is_err());
    }
}
