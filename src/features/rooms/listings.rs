//! Public listings: a slug that names a room anyone may ask to join without an invite. Only the
//! devices the operator names in `LISTING_ADMINS` list rooms, and every join still waits for a
//! room admin.

use buccanet_protocol::api::{JoinRequested, ListingView, SetListing};
use buccanet_protocol::listing::{check_text, valid_slug};
use mongodb::bson::DateTime;

use crate::error::{AppError, AppResult};
use crate::features::devices::repo::DeviceDoc;
use crate::features::rooms::joins::request_into;
use crate::features::rooms::repo::{ListingDoc, RoomDoc};
use crate::features::rooms::service::admin_room;
use crate::state::AppState;

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

/// Lists a room under a slug, or moves the slug to it. A slug stays with the device that listed
/// it first; the room it leaves stops being listed unless another slug names it.
pub async fn set(state: &AppState, room_id: &str, device: &DeviceDoc, input: SetListing) -> AppResult<ListingView> {
    if !state.listing_admins.contains(&device.id) {
        return Err(AppError::forbidden("This relay lists rooms for the devices its operator allows."));
    }
    checked(&input)?;
    let room = admin_room(state, room_id, device).await?;
    let previous = state.rooms.listing(&input.slug).await?;
    if previous.as_ref().is_some_and(|listing| listing.created_by != device.id) {
        return Err(AppError::forbidden("Another device holds this slug."));
    }
    let listing = ListingDoc {
        slug: input.slug,
        room_id: room.id.clone(),
        title: input.title.trim().to_string(),
        description: input.description.trim().to_string(),
        created_by: device.id.clone(),
        updated_at: DateTime::now(),
    };
    state.rooms.save_listing(&listing).await?;
    state.rooms.set_listed(&room.id, true).await?;
    if let Some(left) = previous.filter(|previous| previous.room_id != room.id)
        && !state.rooms.listed_elsewhere(&left.room_id, &left.slug).await?
    {
        state.rooms.set_listed(&left.room_id, false).await?;
    }
    Ok(view(&listing, &room))
}

/// The listing and the room it names.
async fn listed_room(state: &AppState, slug: &str) -> AppResult<(ListingDoc, RoomDoc)> {
    let listing = state.rooms.listing(slug).await?.ok_or_else(|| AppError::not_found("This listing"))?;
    let room = state.rooms.by_id(&listing.room_id).await?.ok_or_else(|| AppError::not_found("This listing"))?;
    Ok((listing, room))
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
    request_into(state, &room, device, key_package).await
}
