//! Public listings: a slug that names a room anyone may ask to join without an invite. An admin
//! of a room lists it, a device holds a few slugs at most, and every join still waits for a room
//! admin. A listing stands while its room exists and the device that listed it is still one of
//! the room's admins; one that no longer stands is deleted when it is read and by the sweep.

use buccanet_protocol::api::{JoinRequested, ListingView, SetListing};
use buccanet_protocol::listing::{check_text, valid_slug};
use mongodb::bson::DateTime;

use crate::error::{AppError, AppResult};
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::joins::{Requested, request_into};
use crate::features::rooms::repo::{ListingDoc, RequestVia, RoomDoc};
use crate::features::rooms::service::admin_room;
use crate::state::AppState;

/// The slugs one device may hold, so no device fills the listings.
const LISTINGS_PER_DEVICE: usize = 3;

fn checked(input: &SetListing) -> AppResult<()> {
    if !valid_slug(&input.slug) {
        return Err(AppError::invalid("A slug has 1 to 40 lowercase letters, digits and inner hyphens."));
    }
    check_text(&input.title, &input.description).map_err(AppError::invalid)
}

fn view(listing: &ListingDoc, room: &RoomDoc) -> ListingView {
    ListingView {
        slug: listing.slug.clone(),
        room_id: room.id.clone(),
        title: listing.title.clone(),
        description: listing.description.clone(),
        member_count: room.members.len() as u32,
        max_members: room.policy.max_members,
    }
}

/// Whether a listing stands: its room exists and its lister is still an admin of it.
fn stands(listing: &ListingDoc, room: Option<&RoomDoc>) -> bool {
    room.is_some_and(|room| room.member(&listing.created_by).is_some_and(|member| member.admin))
}

/// Deletes a listing that no longer stands; its room, when it still exists, stops being listed
/// unless another slug names it.
async fn expire(state: &AppState, listing: &ListingDoc, room: Option<&RoomDoc>) -> AppResult<()> {
    state.rooms.delete_listing(listing).await?;
    if room.is_some() && !state.rooms.listed_elsewhere(&listing.room_id, &listing.slug).await? {
        state.rooms.set_listed(&listing.room_id, false).await?;
    }
    Ok(())
}

/// The listing under `slug` and its room while the listing stands; one that no longer stands is
/// deleted.
async fn standing(state: &AppState, listing: ListingDoc) -> AppResult<Option<(ListingDoc, RoomDoc)>> {
    let room = state.rooms.by_id(&listing.room_id).await?;
    if !stands(&listing, room.as_ref()) {
        expire(state, &listing, room.as_ref()).await?;
        return Ok(None);
    }
    Ok(room.map(|room| (listing, room)))
}

async fn standing_slug(state: &AppState, slug: &str) -> AppResult<Option<(ListingDoc, RoomDoc)>> {
    match state.rooms.listing(slug).await? {
        Some(listing) => standing(state, listing).await,
        None => Ok(None),
    }
}

/// How many standing slugs the device holds; the others are deleted on the way.
async fn standing_count(state: &AppState, device_id: &str) -> AppResult<usize> {
    let mut count = 0;
    for listing in state.rooms.listings_of(device_id).await? {
        count += usize::from(standing(state, listing).await?.is_some());
    }
    Ok(count)
}

/// Lists a room under a slug, or moves the slug to it. A slug stays with the device that listed
/// it while the listing stands, which means that device is still an admin of the room it leaves;
/// that room stops being listed unless another slug names it. The device's lock covers the count
/// and the insert, so parallel calls cannot pass the cap.
pub async fn set(state: &AppState, room_id: &str, device: &DeviceDoc, input: SetListing) -> AppResult<ListingView> {
    checked(&input)?;
    let room = admin_room(state, room_id, device).await?;
    let _guard = state.lock_device(&device.id).await;
    let previous = standing_slug(state, &input.slug).await?.map(|(listing, _)| listing);
    let listing = ListingDoc {
        slug: input.slug,
        room_id: room.id.clone(),
        title: input.title.trim().to_string(),
        description: input.description.trim().to_string(),
        created_by: device.id.clone(),
        updated_at: DateTime::now(),
    };
    match &previous {
        Some(held) if held.created_by != device.id => return Err(slug_taken()),
        Some(_) => move_listing(state, &listing).await?,
        None => insert_listing(state, device, &listing).await?,
    }
    state.rooms.set_listed(&room.id, true).await?;
    if let Some(left) = previous.filter(|previous| previous.room_id != room.id)
        && !state.rooms.listed_elsewhere(&left.room_id, &left.slug).await?
    {
        state.rooms.set_listed(&left.room_id, false).await?;
    }
    Ok(view(&listing, &room))
}

fn slug_taken() -> AppError {
    AppError::forbidden("Another device holds this slug.")
}

async fn move_listing(state: &AppState, listing: &ListingDoc) -> AppResult<()> {
    match state.rooms.replace_listing(listing).await? {
        true => Ok(()),
        false => Err(slug_taken()),
    }
}

/// Stores a new slug; a slug another device took meanwhile is a duplicate id, refused.
async fn insert_listing(state: &AppState, device: &DeviceDoc, listing: &ListingDoc) -> AppResult<()> {
    if standing_count(state, &device.id).await? >= LISTINGS_PER_DEVICE {
        return Err(AppError::forbidden("This device holds as many listings as a device may."));
    }
    state.rooms.insert_listing(listing).await.map_err(|error| match error.kind.as_ref() {
        mongodb::error::ErrorKind::Write(_) => slug_taken(),
        _ => error.into(),
    })
}

/// Unlists a room: any of its admins deletes every slug that names it.
pub async fn remove(state: &AppState, room_id: &str, device: &DeviceDoc) -> AppResult<()> {
    let room = admin_room(state, room_id, device).await?;
    state.rooms.unlist_room(&room.id).await?;
    state.rooms.set_listed(&room.id, false).await?;
    Ok(())
}

/// The standing listing under `slug` and its room; a listing that no longer stands is not found.
async fn listed_room(state: &AppState, slug: &str) -> AppResult<(ListingDoc, RoomDoc)> {
    standing_slug(state, slug).await?.ok_or_else(|| AppError::not_found("This listing"))
}

pub async fn show(state: &AppState, slug: &str) -> AppResult<ListingView> {
    let (listing, room) = listed_room(state, slug).await?;
    Ok(view(&listing, &room))
}

pub async fn request(state: &AppState, slug: &str, device: &DeviceDoc, key_package: Vec<u8>) -> AppResult<JoinRequested> {
    if !state.limits.join.allow(&device.id) {
        return Err(AppError::rate_limited("Too many join requests from this device."));
    }
    let (_, room) = listed_room(state, slug).await?;
    request_into(state, Requested { room: &room, device, via: RequestVia::Listing }, key_package).await
}

/// Deletes every listing that no longer stands, for the relay's periodic sweep.
pub async fn sweep(state: &AppState) -> AppResult<()> {
    for listing in state.rooms.all_listings().await? {
        standing(state, listing).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use buccanet_protocol::api::RoomPolicy;

    use super::*;
    use crate::features::rooms::repo::MemberDoc;

    fn member(device_id: &str, admin: bool) -> MemberDoc {
        MemberDoc { device_id: device_id.into(), admin, joined_at: DateTime::now(), sealed: 0 }
    }

    fn room(members: Vec<MemberDoc>) -> RoomDoc {
        let policy = RoomPolicy::default();
        RoomDoc {
            id: "r".into(),
            group_id: vec![1],
            policy,
            epoch: 0,
            last_seq: 0,
            agent_run: 0,
            members,
            created_at: DateTime::now(),
            listed: true,
        }
    }

    #[test]
    fn a_listing_stands_while_its_lister_administers_its_room() {
        let listing = ListingDoc {
            slug: "public".into(),
            room_id: "r".into(),
            title: "Public".into(),
            description: String::new(),
            created_by: "door".into(),
            updated_at: DateTime::now(),
        };
        assert!(stands(&listing, Some(&room(vec![member("door", true), member("ada", false)]))));
        assert!(!stands(&listing, Some(&room(vec![member("door", false), member("ada", true)]))));
        assert!(!stands(&listing, Some(&room(vec![member("ada", true)]))));
        assert!(!stands(&listing, None));
    }
}
