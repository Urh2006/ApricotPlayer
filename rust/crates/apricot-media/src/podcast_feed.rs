//! Bounded, entity-safe RSS/Atom normalization for podcast feeds.

use std::collections::BTreeMap;

use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use chrono::DateTime;
use roxmltree::{Document, Node, ParsingOptions};
use serde_json::{Map, Value};
use thiserror::Error;
use url::Url;

const MAX_XML_NODES: u32 = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct PodcastFeedDocument {
    pub title: String,
    pub source_url: Url,
    pub site_url: Option<Url>,
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PodcastFeedParseError {
    #[error("podcast feed URL must use HTTP or HTTPS")]
    InvalidUrl,
    #[error("podcast feed is not valid safe XML")]
    InvalidXml,
    #[error("podcast feed is neither RSS nor Atom")]
    UnsupportedRoot,
}

/// Parses one complete RSS or Atom document without DTD/entity expansion.
///
/// # Errors
///
/// Returns an error for unsafe URLs, malformed XML, DTD declarations, node
/// limit exhaustion, and unsupported root elements.
pub fn parse_podcast_feed(
    xml: &str,
    source_url: &Url,
) -> Result<PodcastFeedDocument, PodcastFeedParseError> {
    if !is_remote_url(source_url)
        || !source_url.username().is_empty()
        || source_url.password().is_some()
    {
        return Err(PodcastFeedParseError::InvalidUrl);
    }
    let document = Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_XML_NODES,
            ..ParsingOptions::default()
        },
    )
    .map_err(|_| PodcastFeedParseError::InvalidXml)?;
    let root = document.root_element();
    if named(root, "feed") {
        Ok(parse_atom(root, source_url))
    } else if named(root, "rss") || named(root, "rdf") || child(root, "channel").is_some() {
        Ok(parse_rss(root, source_url))
    } else {
        Err(PodcastFeedParseError::UnsupportedRoot)
    }
}

fn parse_rss(root: Node<'_, '_>, source_url: &Url) -> PodcastFeedDocument {
    let channel = child(root, "channel").unwrap_or(root);
    let title = child_text(channel, "title");
    let site_url = absolute_url(&child_text(channel, "link"), source_url);
    let items = children(channel, "item")
        .filter_map(|node| parse_rss_item(node, source_url, &title))
        .collect();
    PodcastFeedDocument {
        title,
        source_url: source_url.clone(),
        site_url,
        items,
    }
}

fn parse_atom(root: Node<'_, '_>, source_url: &Url) -> PodcastFeedDocument {
    let title = child_text(root, "title");
    let site_url = atom_link(root, source_url, &["alternate", ""]);
    let items = children(root, "entry")
        .filter_map(|node| parse_atom_item(node, source_url, &title))
        .collect();
    PodcastFeedDocument {
        title,
        source_url: source_url.clone(),
        site_url,
        items,
    }
}

fn parse_rss_item(node: Node<'_, '_>, base: &Url, feed_title: &str) -> Option<MediaItem> {
    let title = child_text(node, "title");
    let page_url = absolute_url(&child_text(node, "link"), base);
    let media_url = node.children().filter(Node::is_element).find_map(|child| {
        (named(child, "enclosure") || named(child, "content"))
            .then(|| attribute(child, "url"))
            .flatten()
            .and_then(|url| absolute_url(url, base))
    });
    let guid = child_text(node, "guid");
    let durable_url = media_url
        .clone()
        .or_else(|| page_url.clone())
        .or_else(|| absolute_url(&guid, base));
    let published = child_text(node, "pubDate").or_text(child_text(node, "published"));
    let description = child_text(node, "description")
        .or_text(child_text(node, "summary"))
        .or_text(child_text(node, "content"));
    let duration = child_text(node, "duration");
    make_episode(
        node,
        title,
        durable_url,
        page_url,
        media_url,
        guid,
        &published,
        &description,
        &duration,
        feed_title,
        base,
    )
}

fn parse_atom_item(node: Node<'_, '_>, base: &Url, feed_title: &str) -> Option<MediaItem> {
    let title = child_text(node, "title");
    let page_url = atom_link(node, base, &["alternate", ""]);
    let media_url = atom_link(node, base, &["enclosure"]);
    let guid = child_text(node, "id");
    let durable_url = media_url
        .clone()
        .or_else(|| page_url.clone())
        .or_else(|| absolute_url(&guid, base));
    let published = child_text(node, "published").or_text(child_text(node, "updated"));
    let description = child_text(node, "summary").or_text(child_text(node, "content"));
    let duration = child_text(node, "duration");
    make_episode(
        node,
        title,
        durable_url,
        page_url,
        media_url,
        guid,
        &published,
        &description,
        &duration,
        feed_title,
        base,
    )
}

#[allow(clippy::too_many_arguments)]
fn make_episode(
    node: Node<'_, '_>,
    title: String,
    durable_url: Option<Url>,
    page_url: Option<Url>,
    media_url: Option<Url>,
    guid: String,
    published: &str,
    description: &str,
    duration: &str,
    feed_title: &str,
    base: &Url,
) -> Option<MediaItem> {
    let title = if title.trim().is_empty() {
        durable_url.as_ref()?.to_string()
    } else {
        title
    };
    let mut metadata = BTreeMap::new();
    metadata.insert("kind".to_owned(), Value::String("rss_item".to_owned()));
    metadata.insert(
        "type".to_owned(),
        Value::String("Podcast episode".to_owned()),
    );
    metadata.insert(
        "webpage_url".to_owned(),
        Value::String(
            page_url
                .or_else(|| durable_url.clone())
                .map_or_else(String::new, |url| url.to_string()),
        ),
    );
    metadata.insert("guid".to_owned(), Value::String(guid.clone()));
    metadata.insert(
        "media_url".to_owned(),
        Value::String(media_url.map_or_else(String::new, |url| url.to_string())),
    );
    metadata.insert(
        "description".to_owned(),
        Value::String(strip_html(description)),
    );
    metadata.insert("duration".to_owned(), Value::String(duration.to_owned()));
    metadata.insert(
        "timestamp".to_owned(),
        Value::from(parse_timestamp(published).unwrap_or_default()),
    );
    let (chapters_url, chapters_type) = chapter_reference(node, base);
    metadata.insert(
        "chapters_url".to_owned(),
        Value::String(chapters_url.map_or_else(String::new, |url| url.to_string())),
    );
    metadata.insert("chapters_type".to_owned(), Value::String(chapters_type));
    metadata.insert(
        "chapters".to_owned(),
        Value::Array(
            inline_chapters(node)
                .into_iter()
                .map(Value::Object)
                .collect(),
        ),
    );
    let id = if guid.trim().is_empty() {
        durable_url
            .as_ref()
            .map_or_else(|| title.clone(), ToString::to_string)
    } else {
        guid
    };
    Some(MediaItem {
        id: MediaId(id),
        source: MediaSource::Podcast,
        kind: MediaKind::PodcastEpisode,
        title,
        url: durable_url,
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: feed_title.to_owned(),
        duration_seconds: parse_duration(duration),
        metadata,
    })
}

fn atom_link(node: Node<'_, '_>, base: &Url, rels: &[&str]) -> Option<Url> {
    children(node, "link").find_map(|link| {
        let rel = attribute(link, "rel")
            .unwrap_or_default()
            .to_ascii_lowercase();
        rels.iter()
            .any(|candidate| rel == *candidate)
            .then(|| attribute(link, "href"))
            .flatten()
            .and_then(|href| absolute_url(href, base))
    })
}

fn chapter_reference(node: Node<'_, '_>, base: &Url) -> (Option<Url>, String) {
    children(node, "chapters")
        .find_map(|chapters| {
            attribute(chapters, "url")
                .or_else(|| attribute(chapters, "href"))
                .and_then(|value| absolute_url(value, base))
                .map(|url| {
                    (
                        url,
                        attribute(chapters, "type").unwrap_or_default().to_owned(),
                    )
                })
        })
        .map_or_else(|| (None, String::new()), |(url, kind)| (Some(url), kind))
}

fn inline_chapters(node: Node<'_, '_>) -> Vec<Map<String, Value>> {
    let mut chapters = children(node, "chapters")
        .flat_map(|container| children(container, "chapter"))
        .enumerate()
        .filter_map(|(index, chapter)| {
            let start = attribute(chapter, "start")
                .or_else(|| attribute(chapter, "time"))
                .map_or_else(|| child_text(chapter, "start"), str::to_owned);
            let start = parse_chapter_seconds(&start)?;
            let end =
                attribute(chapter, "end").map_or_else(|| child_text(chapter, "end"), str::to_owned);
            let end = parse_chapter_seconds(&end).filter(|end| *end > start);
            let title = attribute(chapter, "title")
                .map(str::to_owned)
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| {
                    let title = child_text(chapter, "title").or_text(node_text(chapter));
                    if title.trim().is_empty() {
                        format!("Chapters {}", index + 1)
                    } else {
                        title
                    }
                });
            let mut value = Map::new();
            value.insert("title".to_owned(), Value::String(title.trim().to_owned()));
            value.insert(
                "start_time".to_owned(),
                Value::from(round_milliseconds(start)),
            );
            if let Some(end) = end {
                value.insert("end_time".to_owned(), Value::from(round_milliseconds(end)));
            }
            Some(value)
        })
        .collect::<Vec<_>>();
    chapters
        .sort_by(|left, right| number(left, "start_time").total_cmp(&number(right, "start_time")));
    chapters
}

fn parse_duration(value: &str) -> Option<f64> {
    parse_chapter_seconds(value)
}

fn parse_chapter_seconds(value: &str) -> Option<f64> {
    let value = value.trim().replace(',', ".");
    if value.is_empty() {
        return None;
    }
    if let Ok(seconds) = value.parse::<f64>() {
        return seconds.is_finite().then_some(seconds.max(0.0));
    }
    let parts = value.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    parts.into_iter().try_fold(0.0, |total, part| {
        let value = part.parse::<f64>().ok()?;
        value.is_finite().then_some(total * 60.0 + value)
    })
}

#[allow(clippy::cast_precision_loss)]
fn parse_timestamp(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc2822(value)
        .or_else(|_| DateTime::parse_from_rfc3339(value))
        .ok()
        .map(|timestamp| timestamp.timestamp_millis() as f64 / 1000.0)
}

fn strip_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => {
                in_tag = true;
                output.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn absolute_url(value: &str, base: &Url) -> Option<Url> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    base.join(value).ok().filter(is_remote_url)
}

fn is_remote_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
}

fn named(node: Node<'_, '_>, expected: &str) -> bool {
    node.is_element() && node.tag_name().name().eq_ignore_ascii_case(expected)
}

fn child<'a>(node: Node<'a, 'a>, expected: &str) -> Option<Node<'a, 'a>> {
    node.children().find(|child| named(*child, expected))
}

fn children<'a>(node: Node<'a, 'a>, expected: &'a str) -> impl Iterator<Item = Node<'a, 'a>> {
    node.children().filter(move |child| named(*child, expected))
}

fn child_text(node: Node<'_, '_>, expected: &str) -> String {
    child(node, expected).map_or_else(String::new, node_text)
}

fn node_text(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(Node::is_text)
        .filter_map(|descendant| descendant.text())
        .collect::<String>()
        .trim()
        .to_owned()
}

fn attribute<'a>(node: Node<'a, 'a>, expected: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name().eq_ignore_ascii_case(expected))
        .map(|attribute| attribute.value().trim())
        .filter(|value| !value.is_empty())
}

fn number(object: &Map<String, Value>, key: &str) -> f64 {
    object.get(key).and_then(Value::as_f64).unwrap_or_default()
}

fn round_milliseconds(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

trait PreferText {
    fn or_text(self, fallback: String) -> String;
}

impl PreferText for String {
    fn or_text(self, fallback: String) -> String {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PodcastFeedParseError, parse_podcast_feed};
    use apricot_core::{MediaKind, MediaSource};

    #[test]
    fn rss_keeps_full_archive_state_and_podcasting_chapters() {
        let source = "https://podcast.example/feed.xml".parse().expect("URL");
        let xml = r#"<?xml version="1.0"?>
            <rss xmlns:podcast="https://podcastindex.org/namespace/1.0">
              <channel>
                <title>Archive</title><link>/show</link>
                <item>
                  <title>Episode One</title>
                  <link>/episodes/one</link>
                  <guid>episode-one</guid>
                  <enclosure url="https://cdn.example/one.mp3" />
                  <pubDate>Tue, 10 Sep 2024 12:30:00 +0000</pubDate>
                  <description><![CDATA[Hello <b>world</b>]]></description>
                  <duration>01:02:03</duration>
                  <podcast:chapters url="/episodes/one.json" type="application/json+chapters">
                    <podcast:chapter start="00:00" title="Intro" />
                    <podcast:chapter start="05:30.5" end="06:00" title="Topic" />
                  </podcast:chapters>
                </item>
              </channel>
            </rss>"#;
        let feed = parse_podcast_feed(xml, &source).expect("parse");
        assert_eq!(feed.title, "Archive");
        assert_eq!(
            feed.site_url.expect("site").as_str(),
            "https://podcast.example/show"
        );
        assert_eq!(feed.items.len(), 1);
        let episode = &feed.items[0];
        assert_eq!(episode.source, MediaSource::Podcast);
        assert_eq!(episode.kind, MediaKind::PodcastEpisode);
        assert_eq!(
            episode.url.as_ref().expect("media").host_str(),
            Some("cdn.example")
        );
        assert_eq!(episode.duration_seconds, Some(3723.0));
        assert_eq!(episode.metadata["description"], "Hello world");
        assert_eq!(episode.metadata["timestamp"], 1_725_971_400.0);
        assert_eq!(
            episode.metadata["chapters"]
                .as_array()
                .expect("chapters")
                .len(),
            2
        );
        assert_eq!(
            episode.metadata["chapters_url"],
            "https://podcast.example/episodes/one.json"
        );
    }

    #[test]
    fn atom_prefers_enclosure_and_accepts_relative_links() {
        let source = "https://podcast.example/feed.atom".parse().expect("URL");
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
          <title>Atom Show</title><link rel="alternate" href="/show" />
          <entry><title>Entry</title><id>tag:example,1</id>
            <link rel="alternate" href="/entry" />
            <link rel="enclosure" href="/media/entry.ogg" />
            <updated>2024-09-10T12:30:00Z</updated>
          </entry></feed>"#;
        let feed = parse_podcast_feed(xml, &source).expect("parse");
        assert_eq!(
            feed.items[0].url.as_ref().expect("URL").as_str(),
            "https://podcast.example/media/entry.ogg"
        );
        assert_eq!(
            feed.items[0].metadata["webpage_url"],
            "https://podcast.example/entry"
        );
    }

    #[test]
    fn dtd_and_unsupported_documents_are_rejected() {
        let source = "https://podcast.example/feed.xml".parse().expect("URL");
        assert_eq!(
            parse_podcast_feed(
                "<!DOCTYPE rss [<!ENTITY x 'boom'>]><rss><channel><title>&x;</title></channel></rss>",
                &source
            ),
            Err(PodcastFeedParseError::InvalidXml)
        );
        assert_eq!(
            parse_podcast_feed("<html><body>not a feed</body></html>", &source),
            Err(PodcastFeedParseError::UnsupportedRoot)
        );
    }

    #[test]
    fn title_only_episodes_remain_visible_but_not_playable() {
        let source = "https://podcast.example/feed.xml".parse().expect("URL");
        let feed = parse_podcast_feed(
            "<rss><channel><title>Feed</title><item><title>Unsafe</title><enclosure url='file:///secret.mp3'/></item></channel></rss>",
            &source,
        )
        .expect("parse");
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.items[0].title, "Unsafe");
        assert!(!feed.items[0].is_playable());
    }
}
