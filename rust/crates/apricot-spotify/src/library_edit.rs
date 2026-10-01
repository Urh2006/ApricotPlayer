//! Changes to the account's Spotify data (`docs/SPOTIFY_PLAN.md` 3.3, 5.1):
//! Liked Songs and the library, playlist items and playlists. Every change
//! uses the request P0 verified live (`docs/spotify-p0/P0_EVIDENCE.md` 4)
//! and names exact occurrences (`uid`), never positions. Saved states are
//! read back after the change, so the UI announces Spotify's state.

#![allow(clippy::missing_errors_doc)] // Every change fails only with `ApiError`.

use std::time::{SystemTime, UNIX_EPOCH};

use librespot_core::Session;
use serde_json::{Value, json};

use crate::{
    api::{Api, ApiError},
    catalog,
};

/// One change the UI asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryEdit {
    /// Like or unlike a track or episode, save or remove an album,
    /// playlist, show, or follow or unfollow an artist: the opposite of the
    /// current state.
    ToggleSaved {
        uri: String,
    },
    AddToPlaylist {
        playlist: String,
        uris: Vec<String>,
    },
    RemoveFromPlaylist {
        playlist: String,
        uids: Vec<String>,
    },
    /// Moves one occurrence before or after another one.
    MoveInPlaylist {
        playlist: String,
        uid: String,
        before: bool,
        other_uid: String,
    },
    CreatePlaylist {
        name: String,
    },
    RenamePlaylist {
        uri: String,
        name: String,
    },
    /// Spotify's "Hide song" (or showing it again), from a personalised
    /// playlist `context`; Spotify keeps it for the whole account.
    Hide {
        uri: String,
        context: String,
        hidden: bool,
    },
}

/// What a change did, as Spotify confirms it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditOutcome {
    Saved { uri: String, saved: bool },
    AddedToPlaylist { playlist: String, count: usize },
    RemovedFromPlaylist { playlist: String, count: usize },
    Moved { playlist: String },
    Created { uri: String, name: String },
    Renamed { uri: String, name: String },
    Hidden { uri: String, hidden: bool },
}

fn playlist_id(uri: &str) -> Result<&str, ApiError> {
    uri.strip_prefix("spotify:playlist:")
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ApiError::Shape(format!("not a playlist: {uri}")))
}

/// Liked Songs (tracks, episodes) or the library (everything else); `saved`
/// is the state Spotify reports afterwards.
pub async fn set_saved(
    api: &Api,
    session: &Session,
    uri: &str,
    saved: bool,
) -> Result<bool, ApiError> {
    let operation = if saved {
        "addToLibrary"
    } else {
        "removeFromLibrary"
    };
    api.pathfinder(session, operation, json!({ "libraryItemUris": [uri] }))
        .await?;
    let confirmed = catalog::saved(api, session, &[uri.to_owned()]).await?;
    confirmed
        .first()
        .copied()
        .ok_or_else(|| ApiError::Shape("no saved state".into()))
}

pub async fn apply(
    api: &Api,
    session: &Session,
    edit: LibraryEdit,
) -> Result<EditOutcome, ApiError> {
    match edit {
        LibraryEdit::ToggleSaved { uri } if uri.starts_with("spotify:playlist:") => {
            let saved = toggle_playlist_in_library(api, session, &uri).await?;
            Ok(EditOutcome::Saved { uri, saved })
        }
        LibraryEdit::ToggleSaved { uri } => {
            let now = catalog::saved(api, session, std::slice::from_ref(&uri))
                .await?
                .first()
                .copied()
                .unwrap_or(false);
            let saved = set_saved(api, session, &uri, !now).await?;
            Ok(EditOutcome::Saved { uri, saved })
        }
        LibraryEdit::AddToPlaylist { playlist, uris } => {
            api.pathfinder(
                session,
                "addToPlaylist",
                json!({
                    "playlistUri": playlist,
                    "playlistItemUris": uris,
                    "newPosition": { "moveType": "BOTTOM_OF_PLAYLIST", "fromUid": null },
                }),
            )
            .await?;
            Ok(EditOutcome::AddedToPlaylist {
                playlist,
                count: uris.len(),
            })
        }
        LibraryEdit::RemoveFromPlaylist { playlist, uids } => {
            api.pathfinder(
                session,
                "removeFromPlaylist",
                json!({ "playlistUri": playlist, "uids": uids }),
            )
            .await?;
            Ok(EditOutcome::RemovedFromPlaylist {
                playlist,
                count: uids.len(),
            })
        }
        LibraryEdit::MoveInPlaylist {
            playlist,
            uid,
            before,
            other_uid,
        } => {
            api.pathfinder(
                session,
                "moveItemsInPlaylist",
                json!({
                    "playlistUri": playlist,
                    "uids": [uid],
                    "newPosition": {
                        "moveType": if before { "BEFORE_UID" } else { "AFTER_UID" },
                        "fromUid": other_uid,
                    },
                }),
            )
            .await?;
            Ok(EditOutcome::Moved { playlist })
        }
        LibraryEdit::CreatePlaylist { name } => {
            let uri = create_playlist(api, session, &name).await?;
            Ok(EditOutcome::Created { uri, name })
        }
        LibraryEdit::RenamePlaylist { uri, name } => {
            rename_playlist(api, session, &uri, &name).await?;
            Ok(EditOutcome::Renamed { uri, name })
        }
        LibraryEdit::Hide {
            uri,
            context,
            hidden,
        } => {
            let mut item = Vec::new();
            crate::api::proto::bytes(&mut item, 1, uri.as_bytes());
            if !hidden {
                crate::api::proto::int(&mut item, 3, 1);
            }
            if !context.is_empty() {
                crate::api::proto::bytes(&mut item, 4, context.as_bytes());
            }
            let mut body = Vec::new();
            crate::api::proto::bytes(&mut body, 1, session.username().as_bytes());
            crate::api::proto::bytes(&mut body, 2, b"ban");
            crate::api::proto::bytes(&mut body, 3, &item);
            api.collection(session, "/collection/v2/write", body)
                .await?;
            let now = catalog::hidden_songs(api, session).await?;
            Ok(EditOutcome::Hidden {
                hidden: now.contains(&uri),
                uri,
            })
        }
    }
}

/// The attribute change the web player sends for a new or renamed playlist.
fn name_ops(name: &str) -> Value {
    json!([{
        "kind": "UPDATE_LIST_ATTRIBUTES",
        "updateListAttributes": {
            "newAttributes": {
                "values": { "name": name, "formatAttributes": [], "pictureSize": [] },
                "noValue": []
            }
        }
    }])
}

/// The library's playlist list (`rootlist`): its revision and URIs in order
/// (folders appear as their start and end entries).
async fn rootlist(api: &Api, session: &Session) -> Result<(String, Vec<String>), ApiError> {
    let answer = api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!(
                "/playlist/v2/user/{}/rootlist?decorate=revision&from=0&length=10000",
                session.username()
            ),
            None,
        )
        .await?;
    let revision = answer
        .get("revision")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::Shape("rootlist without revision".into()))?
        .to_owned();
    let uris = answer
        .pointer("/contents/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.get("uri")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((revision, uris))
}

fn rootlist_change(ops: &Value, base_revision: Option<&str>) -> Value {
    let mut change = json!({
        "deltas": [{ "ops": ops, "info": { "source": { "client": "WEBPLAYER" } } }],
        "wantResultingRevisions": false,
        "wantSyncResult": false,
        "nonces": []
    });
    if let Some(revision) = base_revision {
        change["baseRevision"] = json!(revision);
    }
    change
}

/// Playlists are in the library through the rootlist, not `addToLibrary`.
/// Removing names the exact entry and the revision it was read at, so a
/// change made meanwhile never removes another playlist. Returns whether
/// the playlist is in the library afterwards.
async fn toggle_playlist_in_library(
    api: &Api,
    session: &Session,
    uri: &str,
) -> Result<bool, ApiError> {
    let (revision, uris) = rootlist(api, session).await?;
    let path = format!("/playlist/v2/user/{}/rootlist/changes", session.username());
    let ops = if let Some(index) = uris.iter().position(|entry| entry == uri) {
        let body = rootlist_change(
            &json!([{
                "kind": "REM",
                "rem": { "fromIndex": index, "length": 1, "items": [{ "uri": uri }], "itemsAsKey": true }
            }]),
            Some(&revision),
        );
        api.spclient(session, reqwest::Method::POST, &path, Some(body))
            .await?;
        false
    } else {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        let body = rootlist_change(
            &json!([{
                "kind": "ADD",
                "add": {
                    "items": [{ "uri": uri, "attributes": { "timestamp": timestamp.to_string() } }],
                    "addFirst": true
                }
            }]),
            None,
        );
        api.spclient(session, reqwest::Method::POST, &path, Some(body))
            .await?;
        true
    };
    let (_, now) = rootlist(api, session).await?;
    let saved = now.iter().any(|entry| entry == uri);
    if saved == ops {
        Ok(saved)
    } else {
        Err(ApiError::Shape("the library did not change".into()))
    }
}

/// A new playlist, first in the library (`rootlist`), as the clients do it.
async fn create_playlist(api: &Api, session: &Session, name: &str) -> Result<String, ApiError> {
    let created = api
        .spclient(
            session,
            reqwest::Method::POST,
            "/playlist/v2/playlist",
            Some(json!({ "ops": name_ops(name) })),
        )
        .await?;
    let uri = created
        .get("uri")
        .and_then(Value::as_str)
        .filter(|uri| uri.starts_with("spotify:playlist:"))
        .ok_or_else(|| ApiError::Shape("created playlist without URI".into()))?
        .to_owned();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    api.spclient(
        session,
        reqwest::Method::POST,
        &format!("/playlist/v2/user/{}/rootlist/changes", session.username()),
        Some(json!({
            "deltas": [{
                "ops": [{
                    "kind": "ADD",
                    "add": {
                        "items": [{ "uri": uri, "attributes": { "timestamp": timestamp.to_string() } }],
                        "addFirst": true
                    }
                }],
                "info": { "source": { "client": "WEBPLAYER" } }
            }],
            "wantResultingRevisions": false,
            "wantSyncResult": false,
            "nonces": []
        })),
    )
    .await?;
    Ok(uri)
}

/// Renames a playlist the account may edit, against its current revision.
async fn rename_playlist(
    api: &Api,
    session: &Session,
    uri: &str,
    name: &str,
) -> Result<(), ApiError> {
    let id = playlist_id(uri)?;
    let current = catalog::playlist(api, session, uri, 0).await?;
    if !current.can_edit_metadata {
        return Err(ApiError::Status(403));
    }
    api.spclient(
        session,
        reqwest::Method::POST,
        &format!("/playlist/v2/playlist/{id}/changes"),
        Some(json!({
            "baseRevision": current.revision,
            "deltas": [{ "ops": name_ops(name), "info": { "source": { "client": "WEBPLAYER" } } }],
            "wantResultingRevisions": false,
            "wantSyncResult": false,
            "nonces": []
        })),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_playlist_uris_have_a_playlist_id() {
        assert_eq!(playlist_id("spotify:playlist:abc"), Ok("abc"));
        assert!(playlist_id("spotify:album:abc").is_err());
        assert!(playlist_id("spotify:playlist:").is_err());
    }

    #[test]
    fn a_name_change_is_one_attribute_update() {
        let ops = name_ops("Road trip");
        assert_eq!(ops[0]["kind"], "UPDATE_LIST_ATTRIBUTES");
        assert_eq!(
            ops[0]["updateListAttributes"]["newAttributes"]["values"]["name"],
            "Road trip"
        );
    }
}
