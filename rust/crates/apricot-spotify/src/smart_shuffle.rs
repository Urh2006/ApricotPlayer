//! Smart shuffle (Urh, 2026-10-02): a shuffled playlist with recommended
//! tracks mixed in, as the Spotify apps do. The recommendations come from
//! the playlist extender (`/playlistextender/extendp/`), the source of
//! Spotify's "Recommended" songs for a playlist. They join the upcoming
//! tracks of this Connect device, one after every few playlist tracks, with
//! their own provider, so they can be announced and removed again.

use librespot_protocol::player::ProvidedTrack;
use serde_json::{Value, json};

pub const PROVIDER: &str = "smart_shuffle";
/// One recommendation after this many playlist tracks.
const SPACING: usize = 3;
/// Recommendations asked for at once.
pub const BATCH: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmartTrack {
    pub uri: String,
    pub title: String,
    pub artists: String,
}

/// The request body for `playlist`, without the tracks in `skip` (URIs).
pub fn request(playlist: &str, skip: &[String]) -> Value {
    let skip: Vec<&str> = skip
        .iter()
        .filter_map(|uri| uri.strip_prefix("spotify:track:"))
        .collect();
    json!({
        "playlistURI": playlist,
        "trackSkipIDs": skip,
        "numResults": BATCH,
    })
}

/// The recommended tracks of a playlist extender answer.
pub fn parse(value: &Value) -> Vec<SmartTrack> {
    value
        .get("recommendedTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|track| {
            let uri = track
                .get("originalId")
                .and_then(Value::as_str)
                .filter(|uri| uri.starts_with("spotify:track:"))
                .map(str::to_owned)
                .or_else(|| {
                    track
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|id| format!("spotify:track:{id}"))
                })?;
            let artists = track
                .get("artists")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ");
            Some(SmartTrack {
                uri,
                title: track
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                artists,
            })
        })
        .collect()
}

pub fn is_smart(track: &ProvidedTrack) -> bool {
    track.provider == PROVIDER
}

/// Whether no recommendation is among the upcoming tracks.
pub fn needs_more(next: &[ProvidedTrack]) -> bool {
    !next.iter().any(is_smart)
}

/// The upcoming tracks with `tracks` mixed in: the manually added tracks
/// stay first, then one recommendation after every `SPACING` other tracks.
/// Recommendations already listed are kept; `first_uid` numbers the new ones.
pub fn interleave(
    next: &[ProvidedTrack],
    tracks: &[SmartTrack],
    first_uid: u64,
) -> Vec<ProvidedTrack> {
    let manual = next
        .iter()
        .take_while(|track| track.provider == "queue")
        .count();
    let mut result: Vec<ProvidedTrack> = next[..manual].to_vec();
    let mut recommended = tracks.iter().enumerate();
    let mut since = 0;
    for track in &next[manual..] {
        if is_smart(track) {
            since = 0;
        } else {
            since += 1;
        }
        result.push(track.clone());
        if since == SPACING {
            since = 0;
            if let Some((index, smart)) = recommended.next() {
                result.push(provided(smart, first_uid + index as u64));
            }
        }
    }
    result
}

/// The upcoming tracks without recommendations; `None` when there were none.
pub fn without_smart(next: &[ProvidedTrack]) -> Option<Vec<ProvidedTrack>> {
    if needs_more(next) {
        return None;
    }
    Some(
        next.iter()
            .filter(|track| !is_smart(track))
            .cloned()
            .collect(),
    )
}

fn provided(track: &SmartTrack, uid: u64) -> ProvidedTrack {
    let mut provided = ProvidedTrack {
        uri: track.uri.clone(),
        uid: format!("smart{uid:x}"),
        provider: PROVIDER.to_owned(),
        ..ProvidedTrack::default()
    };
    provided
        .metadata
        .insert("title".to_owned(), track.title.clone());
    provided
        .metadata
        .insert("artist_name".to_owned(), track.artists.clone());
    provided
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(uid: &str) -> ProvidedTrack {
        ProvidedTrack {
            uri: format!("spotify:track:{uid}"),
            uid: uid.to_owned(),
            provider: "context".to_owned(),
            ..ProvidedTrack::default()
        }
    }

    fn smart(id: &str) -> SmartTrack {
        SmartTrack {
            uri: format!("spotify:track:{id}"),
            title: id.to_uppercase(),
            artists: "Artist".to_owned(),
        }
    }

    #[test]
    fn recommendations_follow_every_third_track_after_the_manual_ones() {
        let mut queued = context("q");
        queued.provider = "queue".to_owned();
        let next = vec![
            queued,
            context("a"),
            context("b"),
            context("c"),
            context("d"),
            context("e"),
            context("f"),
            context("g"),
        ];
        let mixed = interleave(&next, &[smart("x"), smart("y"), smart("z")], 1);
        let uids: Vec<&str> = mixed.iter().map(|track| track.uid.as_str()).collect();
        assert_eq!(
            uids,
            ["q", "a", "b", "c", "smart1", "d", "e", "f", "smart2", "g"]
        );
        assert_eq!(mixed[4].uri, "spotify:track:x");
        assert_eq!(mixed[4].metadata["title"], "X");
        assert!(!needs_more(&mixed));
        let plain = without_smart(&mixed).unwrap();
        assert_eq!(plain, next);
        assert!(without_smart(&next).is_none());
    }

    #[test]
    fn extender_answers_and_requests_use_track_ids() {
        let answer = json!({"recommendedTracks": [
            {"id": "5dv", "originalId": "spotify:track:5dv", "name": "Destroy",
             "artists": [{"name": "Zatox"}, {"name": "Other"}]},
            {"id": "6ab", "name": "No original"}
        ]});
        let tracks = parse(&answer);
        assert_eq!(tracks[0].uri, "spotify:track:5dv");
        assert_eq!(tracks[0].artists, "Zatox, Other");
        assert_eq!(tracks[1].uri, "spotify:track:6ab");
        let body = request("spotify:playlist:p", &["spotify:track:5dv".to_owned()]);
        assert_eq!(body["trackSkipIDs"], json!(["5dv"]));
        assert_eq!(body["numResults"], json!(BATCH));
    }
}
