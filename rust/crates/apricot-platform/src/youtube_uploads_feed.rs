//! The newest videos of a `YouTube` channel from its public uploads feed
//! (2.2.0). The `UULF` playlist holds the same videos as the channel's
//! Videos tab that yt-dlp reads, without Shorts and live streams, and one
//! feed takes a fraction of a second instead of a yt-dlp run. A
//! subscription check reads it first and asks yt-dlp only about channels
//! whose newest videos changed.

use std::time::Duration;

/// One client for all feeds of a check.
///
/// # Errors
/// The reason the client could not be built.
pub fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("ApricotPlayer/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())
}

/// The uploads feed of a `UC...` channel id.
#[must_use]
pub fn uploads_feed_url(channel_id: &str) -> Option<String> {
    let rest = channel_id.strip_prefix("UC")?;
    (!rest.is_empty()
        && rest
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')))
    .then(|| format!("https://www.youtube.com/feeds/videos.xml?playlist_id=UULF{rest}"))
}

/// The video ids of a feed, newest first.
#[must_use]
pub fn video_ids_of_feed(xml: &str) -> Vec<String> {
    xml.split("<yt:videoId>")
        .skip(1)
        .filter_map(|part| part.split_once("</yt:videoId>"))
        .map(|(id, _)| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .collect()
}

/// The newest video ids of a channel.
///
/// # Errors
/// The reason the feed could not be read.
pub fn latest_video_ids(
    client: &reqwest::blocking::Client,
    channel_id: &str,
) -> Result<Vec<String>, String> {
    let url = uploads_feed_url(channel_id).ok_or_else(|| "Not a channel id".to_owned())?;
    let response = client.get(url).send().map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let text = response.text().map_err(|error| error.to_string())?;
    Ok(video_ids_of_feed(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uploads_feed_of_a_channel_lists_its_videos() {
        assert_eq!(
            uploads_feed_url("UCfi5kd2Xu_z0rYo93xAyxfA").as_deref(),
            Some("https://www.youtube.com/feeds/videos.xml?playlist_id=UULFfi5kd2Xu_z0rYo93xAyxfA")
        );
        assert_eq!(uploads_feed_url("@handle"), None);
        assert_eq!(uploads_feed_url("UC"), None);
        assert_eq!(uploads_feed_url("UCa&b"), None);
        let xml = "<feed><entry><yt:videoId>first</yt:videoId></entry>\
                   <entry><yt:videoId> second </yt:videoId></entry></feed>";
        assert_eq!(video_ids_of_feed(xml), ["first", "second"]);
        assert!(video_ids_of_feed("<feed></feed>").is_empty());
    }
}
