//! Non-blocking podcast directory and RSS feed work for the native shell.

use std::{
    sync::mpsc::{self, Receiver},
    thread,
};

use apricot_app::{RssFeed, RssRefreshResult};
use apricot_media::{PodcastDirectoryItem, PodcastFeedDocument};
use apricot_platform::{ApplePodcastDirectoryClient, RssClient};

pub(crate) enum PodcastWorkResult {
    FeedAdded(Result<Box<RssFeed>, String>),
    FeedsRefreshed {
        results: Vec<RssRefreshResult>,
        silent: bool,
    },
    DirectorySearched {
        query: String,
        result: Result<Vec<PodcastDirectoryItem>, String>,
    },
    CategoryLoaded {
        category: String,
        result: Result<Vec<PodcastDirectoryItem>, String>,
    },
    FeedsImported {
        feeds: Vec<RssFeed>,
        failures: usize,
    },
}

pub(crate) struct PendingPodcastWork {
    receiver: Receiver<PodcastWorkResult>,
}

impl PendingPodcastWork {
    pub(crate) fn try_recv(&self) -> Result<PodcastWorkResult, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
}

pub(crate) fn add_feed(
    url: String,
    proxy: String,
    unknown_title: String,
    timestamp: f64,
) -> PendingPodcastWork {
    spawn(move || {
        let result = RssClient::new(nonempty(&proxy))
            .and_then(|client| client.fetch(&url))
            .map(|document| feed_from_document(document, &unknown_title, timestamp))
            .map_err(|error| error.to_string());
        PodcastWorkResult::FeedAdded(result.map(Box::new))
    })
}

pub(crate) fn refresh_feeds(
    feeds: Vec<(String, String)>,
    proxy: String,
    unknown_title: String,
    timestamp: f64,
    silent: bool,
) -> PendingPodcastWork {
    spawn(move || {
        let client = RssClient::new(nonempty(&proxy));
        let results = feeds
            .into_iter()
            .map(|(identity_url, request_url)| {
                let result = client
                    .as_ref()
                    .map_err(ToString::to_string)
                    .and_then(|client| {
                        client
                            .fetch(&request_url)
                            .map_err(|error| error.to_string())
                    })
                    .map(|document| feed_from_document(document, &unknown_title, timestamp));
                RssRefreshResult {
                    url: identity_url,
                    result,
                }
            })
            .collect();
        PodcastWorkResult::FeedsRefreshed { results, silent }
    })
}

pub(crate) fn search_directory(
    query: String,
    country: String,
    limit: u32,
    proxy: String,
) -> PendingPodcastWork {
    spawn(move || {
        let result = ApplePodcastDirectoryClient::new(nonempty(&proxy))
            .and_then(|client| client.search(&query, &country, limit))
            .map_err(|error| error.to_string());
        PodcastWorkResult::DirectorySearched { query, result }
    })
}

pub(crate) fn load_category(category: String, genre_id: u32, proxy: String) -> PendingPodcastWork {
    spawn(move || {
        let result = ApplePodcastDirectoryClient::new(nonempty(&proxy))
            .and_then(|client| client.top_by_genre(genre_id))
            .map_err(|error| error.to_string());
        PodcastWorkResult::CategoryLoaded { category, result }
    })
}

pub(crate) fn import_feeds(
    entries: Vec<(String, String)>,
    proxy: String,
    unknown_title: String,
    timestamp: f64,
) -> PendingPodcastWork {
    spawn(move || {
        let client = RssClient::new(nonempty(&proxy));
        let mut feeds = Vec::new();
        let mut failures = 0;
        for (url, fallback_title) in entries {
            let result = client
                .as_ref()
                .map_err(ToString::to_string)
                .and_then(|client| client.fetch(&url).map_err(|error| error.to_string()));
            match result {
                Ok(document) => {
                    let title = if fallback_title.trim().is_empty() {
                        &unknown_title
                    } else {
                        fallback_title.trim()
                    };
                    feeds.push(feed_from_document(document, title, timestamp));
                }
                Err(_) => failures += 1,
            }
        }
        PodcastWorkResult::FeedsImported { feeds, failures }
    })
}

fn spawn(work: impl FnOnce() -> PodcastWorkResult + Send + 'static) -> PendingPodcastWork {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(work());
    });
    PendingPodcastWork { receiver }
}

fn feed_from_document(
    document: PodcastFeedDocument,
    unknown_title: &str,
    timestamp: f64,
) -> RssFeed {
    let title = if document.title.trim().is_empty() {
        unknown_title.to_owned()
    } else {
        document.title
    };
    RssFeed::new(
        title,
        document.source_url.to_string(),
        document
            .site_url
            .map_or_else(String::new, |url| url.to_string()),
        document.items,
        timestamp,
    )
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value.trim())
}

#[cfg(test)]
mod tests {
    use apricot_media::parse_podcast_feed;

    use super::feed_from_document;

    #[test]
    fn fetched_documents_become_complete_python_compatible_archives() {
        let source = "https://feeds.example/show.xml".parse().expect("URL");
        let document = parse_podcast_feed(
            "<rss><channel><item><title>Episode</title><enclosure url='one.mp3'/></item></channel></rss>",
            &source,
        )
        .expect("feed");
        let feed = feed_from_document(document, "Untitled feed", 5.0);
        assert_eq!(feed.title, "Untitled feed");
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.last_checked, Some(5.0));
        assert_eq!(feed.created_at, Some(5.0));
        assert_eq!(feed.items_complete, Some(true));
    }
}
