//! Bounded OPML import and structured export for podcast subscriptions.

use std::{collections::HashSet, io::Cursor};

use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use roxmltree::{Document, ParsingOptions};
use thiserror::Error;
use url::Url;

use crate::{XmlTextError, decode_xml};

pub const MAX_OPML_BYTES: usize = 5_000_000;
const MAX_OPML_NODES: u32 = 100_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpmlFeed {
    pub title: String,
    pub url: Url,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum OpmlParseError {
    #[error("OPML file is larger than the allowed 5 MB limit")]
    TooLarge,
    #[error(transparent)]
    InvalidEncoding(#[from] XmlTextError),
    #[error("OPML file is malformed or exceeds the XML safety limits")]
    InvalidXml,
    #[error("OPML file does not contain any valid HTTP or HTTPS feeds")]
    NoFeeds,
    #[error("OPML export failed")]
    Write,
}

/// Parses and deduplicates RSS outlines from one bounded OPML document.
///
/// # Errors
///
/// Returns an error for oversized, malformed, unsupported, or empty input.
pub fn parse_opml(bytes: &[u8]) -> Result<Vec<OpmlFeed>, OpmlParseError> {
    if bytes.len() > MAX_OPML_BYTES {
        return Err(OpmlParseError::TooLarge);
    }
    let xml = decode_xml(bytes)?;
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_OPML_NODES,
        ..ParsingOptions::default()
    };
    let document =
        Document::parse_with_options(&xml, options).map_err(|_| OpmlParseError::InvalidXml)?;
    if !document
        .root_element()
        .tag_name()
        .name()
        .eq_ignore_ascii_case("opml")
    {
        return Err(OpmlParseError::InvalidXml);
    }
    let mut seen = HashSet::new();
    let feeds = document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case("outline"))
        .filter_map(|node| {
            let raw = node
                .attribute("xmlUrl")
                .or_else(|| node.attribute("xmlurl"))?;
            let url = remote_url(raw)?;
            let identity = url.as_str().trim_end_matches('/').to_ascii_lowercase();
            if !seen.insert(identity) {
                return None;
            }
            let title = node
                .attribute("title")
                .or_else(|| node.attribute("text"))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("RSS Feed")
                .to_owned();
            Some(OpmlFeed { title, url })
        })
        .collect::<Vec<_>>();
    if feeds.is_empty() {
        Err(OpmlParseError::NoFeeds)
    } else {
        Ok(feeds)
    }
}

/// Serializes podcast feed records as a standards-compatible OPML 2.0 file.
///
/// # Errors
///
/// Returns an error if the XML writer rejects any event.
pub fn write_opml(feeds: &[OpmlFeed]) -> Result<Vec<u8>, OpmlParseError> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|_| OpmlParseError::Write)?;
    let mut opml = BytesStart::new("opml");
    opml.push_attribute(("version", "2.0"));
    writer
        .write_event(Event::Start(opml))
        .map_err(|_| OpmlParseError::Write)?;
    writer
        .write_event(Event::Start(BytesStart::new("head")))
        .map_err(|_| OpmlParseError::Write)?;
    writer
        .write_event(Event::Start(BytesStart::new("title")))
        .map_err(|_| OpmlParseError::Write)?;
    writer
        .write_event(Event::Text(BytesText::new(
            "ApricotPlayer RSS Feeds Export",
        )))
        .map_err(|_| OpmlParseError::Write)?;
    writer
        .write_event(Event::End(BytesEnd::new("title")))
        .map_err(|_| OpmlParseError::Write)?;
    writer
        .write_event(Event::End(BytesEnd::new("head")))
        .map_err(|_| OpmlParseError::Write)?;
    writer
        .write_event(Event::Start(BytesStart::new("body")))
        .map_err(|_| OpmlParseError::Write)?;
    let mut parent = BytesStart::new("outline");
    parent.push_attribute(("text", "Apricot RSS Feeds"));
    parent.push_attribute(("title", "Apricot RSS Feeds"));
    writer
        .write_event(Event::Start(parent))
        .map_err(|_| OpmlParseError::Write)?;
    for feed in feeds {
        let mut outline = BytesStart::new("outline");
        outline.push_attribute(("type", "rss"));
        outline.push_attribute(("text", feed.title.as_str()));
        outline.push_attribute(("title", feed.title.as_str()));
        outline.push_attribute(("xmlUrl", feed.url.as_str()));
        outline.push_attribute(("htmlUrl", feed.url.as_str()));
        writer
            .write_event(Event::Empty(outline))
            .map_err(|_| OpmlParseError::Write)?;
    }
    for name in ["outline", "body", "opml"] {
        writer
            .write_event(Event::End(BytesEnd::new(name)))
            .map_err(|_| OpmlParseError::Write)?;
    }
    Ok(writer.into_inner().into_inner())
}

fn remote_url(value: &str) -> Option<Url> {
    let url = Url::parse(value.trim()).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

#[cfg(test)]
mod tests {
    use super::{MAX_OPML_BYTES, OpmlFeed, OpmlParseError, parse_opml, write_opml};

    #[test]
    fn imports_nested_outlines_and_deduplicates_urls() {
        let feeds = parse_opml(
            br"<?xml version='1.0'?><opml version='2.0'><body><outline text='Folder'><outline text='One' xmlUrl='https://feeds.example/one'/><outline text='Duplicate' xmlUrl='https://feeds.example/one/'/><outline xmlUrl='file:///local'/></outline></body></opml>",
        )
        .expect("OPML");
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title, "One");
    }

    #[test]
    fn export_round_trips_escaped_titles() {
        let original = vec![OpmlFeed {
            title: "News & <Analysis>".to_owned(),
            url: "https://feeds.example/show?one=1&two=2"
                .parse()
                .expect("URL"),
        }];
        let bytes = write_opml(&original).expect("write");
        let parsed = parse_opml(&bytes).expect("parse");
        assert_eq!(parsed, original);
    }

    #[test]
    fn rejects_dtd_empty_and_oversized_documents() {
        assert_eq!(
            parse_opml(b"<!DOCTYPE opml><opml><body/></opml>").expect_err("DTD"),
            OpmlParseError::InvalidXml
        );
        assert_eq!(
            parse_opml(b"<opml><body/></opml>").expect_err("empty"),
            OpmlParseError::NoFeeds
        );
        assert_eq!(
            parse_opml(&vec![b'x'; MAX_OPML_BYTES + 1]).expect_err("large"),
            OpmlParseError::TooLarge
        );
    }
}
