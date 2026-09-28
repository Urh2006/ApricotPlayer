//! Python comments view (`show_comments`, `media.py` comment helpers and
//! `fetch_comments_worker`): normalized `YouTube` comments, their list labels,
//! search, sorting, copy text and the paging state of the comments dialog.

use std::sync::OnceLock;

use apricot_core::{MediaItem, TranslationCatalog};
use regex::Regex;
use serde_json::Value;

/// Python keeps at most 20 comments from one yt-dlp extraction.
pub const YTDLP_COMMENT_LIMIT: usize = 20;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub text: String,
    pub published: String,
    pub timestamp: i64,
    /// `None` when the source reported no like count (Python `None`).
    pub likes: Option<i64>,
    pub reply_count: u64,
    pub replies: Vec<Comment>,
    pub author_channel_url: String,
    pub author_channel_id: String,
}

/// One loaded page and the Python source key used in the announcement.
#[derive(Clone, Debug, PartialEq)]
pub struct CommentsPage {
    pub comments: Vec<Comment>,
    pub next_page: String,
    pub source_key: &'static str,
}

/// Python `sort_choices` in the comments dialog, in its order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommentSort {
    #[default]
    Relevance,
    Newest,
    Oldest,
    Likes,
    Replies,
}

impl CommentSort {
    pub const ALL: [Self; 5] = [
        Self::Relevance,
        Self::Newest,
        Self::Oldest,
        Self::Likes,
        Self::Replies,
    ];

    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Relevance => "comments_sort_relevance",
            Self::Newest => "comments_sort_newest",
            Self::Oldest => "comments_sort_oldest",
            Self::Likes => "comments_sort_likes",
            Self::Replies => "comments_sort_replies",
        }
    }
}

/// Python `youtube_comments_source_url`: a stored `YouTube` URL of the same
/// video, or the canonical watch URL.
#[must_use]
pub fn source_url(item: &MediaItem, video_id: &str) -> String {
    let stored = ["webpage_url", "original_url", "watch_url"]
        .iter()
        .filter_map(|key| item.metadata.get(*key).and_then(Value::as_str))
        .map(str::to_owned)
        .chain(item.url.as_ref().map(ToString::to_string));
    for candidate in stored {
        let candidate = candidate.trim();
        let id = MediaItem::from_direct_link(candidate).and_then(|probe| probe.youtube_video_id());
        if id.as_deref() == Some(video_id) {
            return candidate.to_owned();
        }
    }
    format!("https://www.youtube.com/watch?v={video_id}")
}

/// Python `normalize_youtube_comment_thread` for one `commentThreads` item.
#[must_use]
pub fn comment_from_api_thread(item: &Value) -> Comment {
    let snippet = item.get("snippet");
    let top = snippet.and_then(|snippet| snippet.get("topLevelComment"));
    let mut comment = comment_from_api_snippet(top.and_then(|top| top.get("snippet")));
    comment.id = text_value(top.and_then(|top| top.get("id")));
    if comment.id.is_empty() {
        comment.id = text_value(item.get("id"));
    }
    comment.reply_count = count_value(snippet.and_then(|snippet| snippet.get("totalReplyCount")));
    comment.replies = item
        .pointer("/replies/comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|reply| reply.is_object())
        .map(|reply| {
            let mut data = comment_from_api_snippet(reply.get("snippet"));
            data.id = text_value(reply.get("id"));
            data
        })
        .filter(|reply| !reply.text.is_empty())
        .collect();
    comment
}

/// Python `fetch_youtube_comments` after the API request: comments with text
/// and the next page token.
#[must_use]
pub fn page_from_api_payload(payload: &Value) -> (Vec<Comment>, String) {
    let comments = payload
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.is_object())
        .map(comment_from_api_thread)
        .filter(|comment| !comment.text.is_empty())
        .collect();
    (comments, text_value(payload.get("nextPageToken")))
}

/// Python `normalize_comment_snippet`.
fn comment_from_api_snippet(snippet: Option<&Value>) -> Comment {
    let field = |key: &str| snippet.and_then(|snippet| snippet.get(key));
    let original = text_value(field("textOriginal"));
    let raw = if original.is_empty() {
        text_value(field("textDisplay"))
    } else {
        original
    };
    let text = html_escape::decode_html_entities(&strip_html(&raw)).into_owned();
    let channel_id = match field("authorChannelId") {
        Some(Value::Object(object)) => text_value(object.get("value")).trim().to_owned(),
        other => text_value(other).trim().to_owned(),
    };
    let mut author_channel_url = text_value(field("authorChannelUrl")).trim().to_owned();
    if author_channel_url.is_empty() && !channel_id.is_empty() {
        author_channel_url = format!("https://www.youtube.com/channel/{channel_id}");
    }
    let published = text_value(field("publishedAt")).trim().to_owned();
    Comment {
        id: String::new(),
        author: text_value(field("authorDisplayName")).trim().to_owned(),
        text: text.trim().to_owned(),
        timestamp: timestamp_from_iso_datetime(&published).unwrap_or(0),
        published,
        likes: likes_value(snippet, "likeCount"),
        reply_count: 0,
        replies: Vec::new(),
        author_channel_url,
        author_channel_id: channel_id,
    }
}

/// Python `fetch_ytdlp_comments` for one extracted info dictionary.
/// `format_time` is Python `format_history_time` (local time).
#[must_use]
pub fn comments_from_ytdlp_info(info: &Value, format_time: impl Fn(i64) -> String) -> Vec<Comment> {
    let mut comments = Vec::new();
    for raw in info
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(YTDLP_COMMENT_LIMIT)
    {
        if !raw.is_object() {
            continue;
        }
        let text = strip_html(&text_value(raw.get("text")));
        if text.is_empty() {
            continue;
        }
        let mut author_channel_url = text_value(raw.get("author_url")).trim().to_owned();
        let author_id = text_value(raw.get("author_id")).trim().to_owned();
        if !author_channel_url.is_empty() && !author_channel_url.starts_with("http") {
            author_channel_url = format!(
                "https://www.youtube.com/{}",
                author_channel_url.trim_start_matches('/')
            );
        }
        if author_channel_url.is_empty() && author_id.starts_with("UC") {
            author_channel_url = format!("https://www.youtube.com/channel/{author_id}");
        }
        let mut author = text_value(raw.get("author"));
        if author.is_empty() {
            author.clone_from(&author_id);
        }
        let timestamp = raw.get("timestamp").and_then(Value::as_f64).unwrap_or(0.0);
        #[allow(clippy::cast_possible_truncation)]
        let whole = timestamp as i64;
        comments.push(Comment {
            id: text_value(raw.get("id")),
            author: author.trim().to_owned(),
            text,
            published: if timestamp == 0.0 {
                String::new()
            } else {
                format_time(whole)
            },
            timestamp: whole.max(0),
            likes: likes_value(Some(raw), "like_count"),
            reply_count: 0,
            replies: Vec::new(),
            author_channel_url,
            author_channel_id: author_id,
        });
    }
    comments
}

/// Python `format_history_time`: local `%Y-%m-%d %H:%M`.
#[must_use]
pub fn format_history_time(timestamp: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// Python `comment_line`.
#[must_use]
pub fn comment_line(catalog: &TranslationCatalog, comment: &Comment, index: usize) -> String {
    let mut text = comment
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > 140 {
        text = format!(
            "{}...",
            text.chars().take(137).collect::<String>().trim_end()
        );
    }
    let author = if comment.author.is_empty() {
        catalog.text("comments")
    } else {
        &comment.author
    };
    let mut parts = vec![format!("{}. {author}", index + 1), text];
    let likes = format_count(comment.likes);
    if !likes.is_empty() {
        parts.push(catalog.text("comment_likes").replace("{count}", &likes));
    }
    if comment.reply_count > 0 {
        parts.push(
            catalog
                .text("comment_replies_count")
                .replace("{count}", &comment.reply_count.to_string()),
        );
    }
    parts.retain(|part| !part.is_empty());
    parts.join(" | ")
}

/// Python `comments_sorted`; stable like Python `sorted`.
#[must_use]
pub fn sorted_comments<'a>(comments: &[&'a Comment], sort: CommentSort) -> Vec<&'a Comment> {
    let mut items = comments.to_vec();
    sort_by_mode(&mut items, sort, |comment| *comment);
    items
}

fn sort_by_mode<'c, T>(items: &mut [T], sort: CommentSort, comment: impl Fn(&T) -> &'c Comment) {
    use std::cmp::Reverse;
    match sort {
        CommentSort::Relevance => {}
        CommentSort::Newest => items.sort_by_key(|item| Reverse(comment(item).timestamp)),
        CommentSort::Oldest => items.sort_by_key(|item| comment(item).timestamp),
        CommentSort::Likes => items.sort_by_key(|item| Reverse(comment(item).likes.unwrap_or(0))),
        CommentSort::Replies => items.sort_by_key(|item| Reverse(comment(item).reply_count)),
    }
}

/// Python `comment_matches_query`.
#[must_use]
pub fn comment_matches_query(comment: &Comment, query: &str) -> bool {
    let normalized = query.trim().to_lowercase();
    if normalized.is_empty() {
        return true;
    }
    let mut parts = vec![
        comment.author.as_str(),
        comment.text.as_str(),
        comment.published.as_str(),
    ];
    for reply in &comment.replies {
        parts.push(&reply.author);
        parts.push(&reply.text);
    }
    parts.join(" ").to_lowercase().contains(&normalized)
}

/// Python `comment_copy_text`.
#[must_use]
pub fn comment_copy_text(
    catalog: &TranslationCatalog,
    comment: &Comment,
    index: Option<usize>,
) -> String {
    let prefix = index
        .map(|index| format!("{}. ", index + 1))
        .unwrap_or_default();
    let author = if comment.author.is_empty() {
        catalog.text("comments")
    } else {
        &comment.author
    };
    let mut lines = vec![
        format!("{prefix}{author}"),
        comment.published.clone(),
        likes_line(catalog, comment),
        String::new(),
        comment.text.clone(),
    ];
    push_replies(catalog, &mut lines, &comment.replies);
    lines.join("\n").trim().to_owned()
}

/// Python `comments_copy_text`.
#[must_use]
pub fn comments_copy_text(catalog: &TranslationCatalog, comments: &[&Comment]) -> String {
    comments
        .iter()
        .enumerate()
        .map(|(index, comment)| comment_copy_text(catalog, comment, Some(index)))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
        .trim()
        .to_owned()
}

/// Python `comment_details_text`.
#[must_use]
pub fn comment_details_text(catalog: &TranslationCatalog, comment: &Comment) -> String {
    let mut lines = vec![
        comment.author.clone(),
        comment.published.clone(),
        likes_line(catalog, comment),
        String::new(),
        comment.text.clone(),
    ];
    push_replies(catalog, &mut lines, &comment.replies);
    let shown = u64::try_from(comment.replies.len()).unwrap_or(u64::MAX);
    if comment.reply_count > shown {
        lines.push(String::new());
        lines.push(
            catalog
                .text("comment_more_replies")
                .replace("{count}", &(comment.reply_count - shown).to_string()),
        );
    }
    lines.join("\n")
}

fn likes_line(catalog: &TranslationCatalog, comment: &Comment) -> String {
    comment.likes.map_or_else(String::new, |likes| {
        catalog
            .text("comment_likes")
            .replace("{count}", &format_count(Some(likes)))
    })
}

fn push_replies(catalog: &TranslationCatalog, lines: &mut Vec<String>, replies: &[Comment]) {
    if replies.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(catalog.text("comment_replies").to_owned());
    for reply in replies {
        lines.push(String::new());
        lines.push(reply.author.clone());
        lines.push(reply.text.clone());
    }
}

/// Python `format_count`, including its rounding (`1999` is `2.0K`).
#[must_use]
pub fn format_count(value: Option<i64>) -> String {
    let Some(number) = value else {
        return String::new();
    };
    #[allow(clippy::cast_precision_loss)]
    let float = number as f64;
    if number >= 1_000_000_000 {
        format!("{:.1}B", float / 1_000_000_000.0)
    } else if number >= 1_000_000 {
        format!("{:.1}M", float / 1_000_000.0)
    } else if number >= 1_000 {
        format!("{:.1}K", float / 1_000.0)
    } else {
        number.to_string()
    }
}

/// Python `friendly_error` hints for cookie and sign-in failures.
#[must_use]
pub fn friendly_error(catalog: &TranslationCatalog, text: &str) -> String {
    let lowered = text.to_lowercase();
    if lowered.contains("failed to decrypt with dpapi")
        || (lowered.contains("nonetype") && lowered.contains("decode"))
        || (lowered.contains("could not copy")
            && lowered.contains("cookie")
            && lowered.contains("database"))
    {
        return format!("{text}\n\n{}", catalog.text("cookie_copy_hint"));
    }
    if lowered.contains("sign in to confirm")
        || lowered.contains("not a bot")
        || lowered.contains("cookies-from-browser")
    {
        return format!("{text}\n\n{}", catalog.text("youtube_auth_hint"));
    }
    text.to_owned()
}

/// One Data API request for a page token.
pub type ApiRequest<'a> = dyn Fn(&str) -> Result<Value, String> + 'a;

/// Python `fetch_comments_worker`: the Data API when a key is configured,
/// otherwise (or after a failed first page) yt-dlp. `api` is `None` without
/// a key. Errors are already Python `friendly_error` text.
///
/// # Errors
/// Returns the failure text, joining a different API error before it.
pub fn fetch_page(
    catalog: &TranslationCatalog,
    page_token: &str,
    api: Option<&ApiRequest<'_>>,
    ytdlp: &dyn Fn() -> Result<Value, String>,
) -> Result<CommentsPage, String> {
    let mut api_error = String::new();
    if let Some(api) = api {
        match api(page_token) {
            Ok(payload) => {
                let (comments, next_page) = page_from_api_payload(&payload);
                return Ok(CommentsPage {
                    comments,
                    next_page,
                    source_key: "comments_source_api",
                });
            }
            Err(error) => {
                api_error = friendly_error(catalog, &error);
                if !page_token.is_empty() {
                    return Err(api_error);
                }
            }
        }
    }
    match ytdlp() {
        Ok(info) => Ok(CommentsPage {
            comments: comments_from_ytdlp_info(&info, format_history_time),
            next_page: String::new(),
            source_key: "comments_source_ytdlp",
        }),
        Err(error) => {
            let error = friendly_error(catalog, &error);
            if !api_error.is_empty() && api_error != error {
                Err(format!("{api_error}\n\n{error}"))
            } else {
                Err(error)
            }
        }
    }
}

/// What the dialog shows in its list and which commands are enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommentsListView {
    pub labels: Vec<String>,
    pub selection: usize,
    /// Open, copy and copy visible.
    pub has_comments: bool,
    pub author_enabled: bool,
    pub more_enabled: bool,
}

/// Python `load_more` before the worker starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadRequest {
    Ignored,
    NoMore,
    /// `loading_label` replaces the list while no comment is loaded yet.
    Start {
        page_token: String,
        loading_label: Option<String>,
    },
}

/// Python `finish_load`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadFinished {
    /// The list shows only `label`; only "Load more" is disabled.
    Failed { label: String, announcement: String },
    Loaded {
        view: CommentsListView,
        announcement: String,
    },
}

/// Python `state` of the comments dialog.
#[derive(Clone, Debug, Default)]
pub struct CommentsState {
    comments: Vec<Comment>,
    visible: Vec<usize>,
    next_page: String,
    loading: bool,
    loaded_once: bool,
    source_key: &'static str,
    sort: CommentSort,
    query: String,
}

impl CommentsState {
    #[must_use]
    pub fn loading(&self) -> bool {
        self.loading
    }

    pub fn set_query(&mut self, query: &str) {
        query.clone_into(&mut self.query);
    }

    pub fn set_sort(&mut self, sort: CommentSort) {
        self.sort = sort;
    }

    /// Python `selected_comment`.
    #[must_use]
    pub fn selected(&self, row: usize) -> Option<&Comment> {
        self.visible.get(row).map(|index| &self.comments[*index])
    }

    /// Python `state["visible_comments"]`.
    #[must_use]
    pub fn visible_comments(&self) -> Vec<&Comment> {
        self.visible
            .iter()
            .map(|index| &self.comments[*index])
            .collect()
    }

    #[must_use]
    pub fn query_active(&self) -> bool {
        !self.query.trim().is_empty()
    }

    #[must_use]
    pub fn more_enabled(&self) -> bool {
        !self.next_page.is_empty() && !self.loading
    }

    /// Python's message when copy finds no comment.
    #[must_use]
    pub fn empty_message_key(&self) -> &'static str {
        if self.query_active() {
            "no_matching_comments"
        } else {
            "comments_disabled"
        }
    }

    /// Python `refresh_comments`.
    pub fn refresh(&mut self, catalog: &TranslationCatalog, selection: usize) -> CommentsListView {
        let mut visible = (0..self.comments.len())
            .filter(|index| comment_matches_query(&self.comments[*index], &self.query))
            .collect::<Vec<_>>();
        let comments = &self.comments;
        sort_by_mode(&mut visible, self.sort, |index| &comments[*index]);
        self.visible = visible;
        let placeholder = if self.query_active() && !self.comments.is_empty() {
            "no_matching_comments"
        } else {
            "comments_disabled"
        };
        let mut labels = self
            .visible
            .iter()
            .enumerate()
            .map(|(row, index)| comment_line(catalog, &self.comments[*index], row))
            .collect::<Vec<_>>();
        if labels.is_empty() {
            labels.push(catalog.text(placeholder).to_owned());
        }
        let selection = selection.min(labels.len() - 1);
        CommentsListView {
            author_enabled: self
                .selected(selection)
                .is_some_and(|comment| !comment.author_channel_url.trim().is_empty()),
            has_comments: !self.visible.is_empty(),
            more_enabled: self.more_enabled(),
            labels,
            selection,
        }
    }

    /// Python `load_more`.
    pub fn request_load(&mut self, catalog: &TranslationCatalog) -> LoadRequest {
        if self.loading {
            return LoadRequest::Ignored;
        }
        if (!self.comments.is_empty() || self.loaded_once) && self.next_page.is_empty() {
            return LoadRequest::NoMore;
        }
        self.loading = true;
        LoadRequest::Start {
            page_token: self.next_page.clone(),
            loading_label: self
                .comments
                .is_empty()
                .then(|| catalog.text("comments_loading").to_owned()),
        }
    }

    /// Python `finish_load`.
    pub fn finish_load(
        &mut self,
        catalog: &TranslationCatalog,
        result: Result<CommentsPage, String>,
    ) -> LoadFinished {
        self.loading = false;
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                let text = catalog.text("comments_failed").replace("{error}", &error);
                return LoadFinished::Failed {
                    label: text.clone(),
                    announcement: text,
                };
            }
        };
        let existing = self.comments.len();
        self.comments.extend(page.comments);
        self.next_page = page.next_page;
        if !page.source_key.is_empty() {
            self.source_key = page.source_key;
        }
        self.loaded_once = true;
        let view = self.refresh(catalog, existing);
        let total = self.comments.len();
        let announcement = if total == 0 {
            catalog.text("comments_disabled").to_owned()
        } else if self.source_key.is_empty() {
            catalog
                .text("comments_loaded")
                .replace("{count}", &total.to_string())
        } else {
            catalog
                .text("comments_loaded_from_source")
                .replace("{count}", &total.to_string())
                .replace("{source}", catalog.text(self.source_key))
        };
        LoadFinished::Loaded { view, announcement }
    }
}

/// Python `strip_html` from `apricot/ui/misc.py`.
///
/// # Panics
/// Never: the patterns are constant.
#[must_use]
pub fn strip_html(value: &str) -> String {
    static PATTERNS: OnceLock<[Regex; 4]> = OnceLock::new();
    let [br, tag, inline, newline] = PATTERNS.get_or_init(|| {
        [
            Regex::new(r"(?i)<br\s*/?>").expect("br pattern"),
            Regex::new(r"<[^>]+>").expect("tag pattern"),
            Regex::new(r"[ \t]+").expect("space pattern"),
            Regex::new(r"\n\s+").expect("newline pattern"),
        ]
    });
    let text = br.replace_all(value, "\n");
    let text = tag.replace_all(&text, " ");
    let text = inline.replace_all(&text, " ");
    let text = newline.replace_all(&text, "\n");
    text.trim().to_owned()
}

/// Python `timestamp_from_iso_datetime` for the API's RFC 3339 times.
fn timestamp_from_iso_datetime(value: &str) -> Option<i64> {
    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(parsed.timestamp());
    }
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|parsed| parsed.and_utc().timestamp())
        .or_else(|| {
            chrono::DateTime::parse_from_rfc2822(text)
                .ok()
                .map(|parsed| parsed.timestamp())
        })
}

/// Python `str(value or "")`.
fn text_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(true)) => "True".to_owned(),
        _ => String::new(),
    }
}

/// Python `to_int(str(value or 0), 0, 0)`.
fn count_value(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// Python `get(key, 0)`: a missing key is 0, JSON `null` is `None`.
fn likes_value(parent: Option<&Value>, key: &str) -> Option<i64> {
    match parent.and_then(|parent| parent.get(key)) {
        Some(Value::Null) => None,
        Some(Value::Number(number)) => number.as_i64().or_else(|| {
            #[allow(clippy::cast_possible_truncation)]
            number.as_f64().map(|value| value.trunc() as i64)
        }),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => Some(0),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::embedded_catalog;

    fn comment(author: &str, text: &str) -> Comment {
        Comment {
            author: author.to_owned(),
            text: text.to_owned(),
            likes: Some(0),
            ..Comment::default()
        }
    }

    #[test]
    fn counts_round_like_python_format() {
        assert_eq!(format_count(None), "");
        assert_eq!(format_count(Some(0)), "0");
        assert_eq!(format_count(Some(999)), "999");
        assert_eq!(format_count(Some(1_999)), "2.0K");
        assert_eq!(format_count(Some(1_250)), "1.2K");
        assert_eq!(format_count(Some(1_350)), "1.4K");
        assert_eq!(format_count(Some(2_500_000)), "2.5M");
        assert_eq!(format_count(Some(3_000_000_000)), "3.0B");
    }

    #[test]
    fn api_threads_are_normalized_like_python() {
        let payload = json!({
            "nextPageToken": "NEXT",
            "items": [
                {
                    "id": "thread",
                    "snippet": {
                        "totalReplyCount": 3,
                        "topLevelComment": {
                            "id": "top",
                            "snippet": {
                                "textOriginal": "Great <b>song</b> &amp; video<br>second",
                                "authorDisplayName": " Ana ",
                                "authorChannelId": {"value": "UCabc"},
                                "publishedAt": "2024-01-02T03:04:05Z",
                                "likeCount": 1500
                            }
                        }
                    },
                    "replies": {"comments": [
                        {"id": "r1", "snippet": {"textDisplay": "Agreed", "authorDisplayName": "Bo"}},
                        {"id": "r2", "snippet": {"textDisplay": "", "authorDisplayName": "Empty"}}
                    ]}
                },
                {"snippet": {"topLevelComment": {"snippet": {"textOriginal": "  "}}}}
            ]
        });
        let (comments, next) = page_from_api_payload(&payload);
        assert_eq!(next, "NEXT");
        assert_eq!(comments.len(), 1);
        let first = &comments[0];
        assert_eq!(first.id, "top");
        assert_eq!(first.author, "Ana");
        assert_eq!(first.text, "Great song & video\nsecond");
        assert_eq!(first.published, "2024-01-02T03:04:05Z");
        assert_eq!(first.timestamp, 1_704_164_645);
        assert_eq!(first.likes, Some(1500));
        assert_eq!(first.reply_count, 3);
        assert_eq!(
            first.author_channel_url,
            "https://www.youtube.com/channel/UCabc"
        );
        assert_eq!(first.replies.len(), 1);
        assert_eq!(first.replies[0].author, "Bo");
        assert_eq!(first.replies[0].likes, Some(0));
    }

    #[test]
    fn ytdlp_comments_keep_twenty_and_build_channel_urls() {
        let mut raw = vec![
            json!({"id": "a", "text": "<p>Hi</p>", "author": "", "author_id": "UCxyz",
                   "timestamp": 1_700_000_000, "like_count": null}),
            json!({"id": "b", "text": "Two", "author": "B", "author_url": "/@bee"}),
            json!({"id": "c", "text": "   "}),
        ];
        for index in 0..30 {
            raw.push(json!({"id": index.to_string(), "text": "more"}));
        }
        let comments = comments_from_ytdlp_info(&json!({"comments": raw}), |ts| format!("t{ts}"));
        // Python slices 20 raw entries before skipping empty text.
        assert_eq!(comments.len(), 19);
        assert_eq!(comments[0].author, "UCxyz");
        assert_eq!(comments[0].text, "Hi");
        assert_eq!(comments[0].published, "t1700000000");
        assert_eq!(comments[0].likes, None);
        assert_eq!(
            comments[0].author_channel_url,
            "https://www.youtube.com/channel/UCxyz"
        );
        assert_eq!(
            comments[1].author_channel_url,
            "https://www.youtube.com/@bee"
        );
        assert_eq!(comments[1].published, "");
        assert_eq!(comments[1].likes, Some(0));
    }

    #[test]
    fn list_lines_shorten_text_and_add_counts() {
        let catalog = embedded_catalog("en");
        let mut long = comment("", &format!("{}   end", "word ".repeat(40)));
        long.likes = Some(1_234);
        long.reply_count = 2;
        let line = comment_line(&catalog, &long, 0);
        assert!(line.starts_with("1. Comments | word word"));
        assert!(line.ends_with("... | 1.2K likes | 2 replies"));
        let short = comment("Ana", "Hello\n  there");
        assert_eq!(
            comment_line(&catalog, &short, 4),
            "5. Ana | Hello there | 0 likes"
        );
        let unknown = Comment {
            likes: None,
            ..comment("Ana", "Hi")
        };
        assert_eq!(comment_line(&catalog, &unknown, 0), "1. Ana | Hi");
    }

    #[test]
    fn copy_and_details_text_match_python_layout() {
        let catalog = embedded_catalog("en");
        let mut item = comment("Ana", "Body");
        item.published = "2024".to_owned();
        item.likes = Some(5);
        item.reply_count = 3;
        item.replies = vec![comment("Bo", "Reply")];
        assert_eq!(
            comment_copy_text(&catalog, &item, None),
            "Ana\n2024\n5 likes\n\nBody\n\nReplies\n\nBo\nReply"
        );
        assert_eq!(
            comment_details_text(&catalog, &item),
            "Ana\n2024\n5 likes\n\nBody\n\nReplies\n\nBo\nReply\n\n2 more replies are available on YouTube."
        );
        let plain = Comment {
            likes: None,
            ..comment("", "Text")
        };
        assert_eq!(
            comment_copy_text(&catalog, &plain, Some(1)),
            "2. Comments\n\n\n\nText"
        );
        assert_eq!(
            comments_copy_text(&catalog, &[&plain, &plain]),
            "1. Comments\n\n\n\nText\n\n---\n\n2. Comments\n\n\n\nText"
        );
    }

    #[test]
    fn search_covers_replies_and_sorting_is_stable() {
        let mut first = comment("Ana", "one");
        first.timestamp = 10;
        first.likes = Some(1);
        first.replies = vec![comment("Zed", "hidden reply")];
        let mut second = comment("Bo", "two");
        second.timestamp = 20;
        second.likes = Some(1);
        second.reply_count = 4;
        let mut third = comment("Cy", "three");
        third.timestamp = 5;
        third.likes = None;
        assert!(comment_matches_query(&first, " HIDDEN "));
        assert!(comment_matches_query(&first, "zed"));
        assert!(!comment_matches_query(&second, "hidden"));
        let all = [&first, &second, &third];
        let authors = |sort| {
            sorted_comments(&all, sort)
                .iter()
                .map(|comment| comment.author.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(authors(CommentSort::Relevance), ["Ana", "Bo", "Cy"]);
        assert_eq!(authors(CommentSort::Newest), ["Bo", "Ana", "Cy"]);
        assert_eq!(authors(CommentSort::Oldest), ["Cy", "Ana", "Bo"]);
        assert_eq!(authors(CommentSort::Likes), ["Ana", "Bo", "Cy"]);
        assert_eq!(authors(CommentSort::Replies), ["Bo", "Ana", "Cy"]);
    }

    #[test]
    fn dialog_state_follows_python_load_and_refresh() {
        let catalog = embedded_catalog("en");
        let mut state = CommentsState::default();
        assert_eq!(
            state.request_load(&catalog),
            LoadRequest::Start {
                page_token: String::new(),
                loading_label: Some("Loading comments...".to_owned()),
            }
        );
        assert_eq!(state.request_load(&catalog), LoadRequest::Ignored);
        // Python shows "disabled" while loading even when a query is typed.
        state.set_query("x");
        assert_eq!(
            state.refresh(&catalog, 0).labels,
            ["Comments are disabled or unavailable for this video."]
        );
        state.set_query("");
        let mut with_channel = comment("Ana", "first");
        with_channel.author_channel_url = "https://www.youtube.com/@ana".to_owned();
        let finished = state.finish_load(
            &catalog,
            Ok(CommentsPage {
                comments: vec![with_channel, comment("Bo", "second")],
                next_page: "P2".to_owned(),
                source_key: "comments_source_api",
            }),
        );
        let LoadFinished::Loaded { view, announcement } = finished else {
            panic!("loaded");
        };
        assert_eq!(announcement, "Loaded 2 comments from YouTube Data API.");
        assert_eq!(view.selection, 0);
        assert!(view.has_comments && view.author_enabled && view.more_enabled);
        assert_eq!(
            state.request_load(&catalog),
            LoadRequest::Start {
                page_token: "P2".to_owned(),
                loading_label: None,
            }
        );
        let finished = state.finish_load(
            &catalog,
            Ok(CommentsPage {
                comments: vec![comment("Cy", "third")],
                next_page: String::new(),
                source_key: "comments_source_api",
            }),
        );
        let LoadFinished::Loaded { view, .. } = finished else {
            panic!("loaded");
        };
        // The first new comment is selected.
        assert_eq!(view.selection, 2);
        assert!(!view.more_enabled && !view.author_enabled);
        assert_eq!(state.request_load(&catalog), LoadRequest::NoMore);
        state.set_query("zzz");
        let view = state.refresh(&catalog, 0);
        assert_eq!(view.labels, ["No matching comments."]);
        assert!(!view.has_comments);
        assert_eq!(state.empty_message_key(), "no_matching_comments");
        state.set_query("");
        state.set_sort(CommentSort::Oldest);
        assert_eq!(state.refresh(&catalog, 1).selection, 1);
        assert_eq!(state.selected(1).map(|c| c.author.as_str()), Some("Bo"));
    }

    #[test]
    fn empty_and_failed_loads_use_python_messages() {
        let catalog = embedded_catalog("en");
        let mut state = CommentsState::default();
        let _ = state.request_load(&catalog);
        let LoadFinished::Loaded { view, announcement } = state.finish_load(
            &catalog,
            Ok(CommentsPage {
                comments: Vec::new(),
                next_page: String::new(),
                source_key: "comments_source_ytdlp",
            }),
        ) else {
            panic!("loaded");
        };
        assert_eq!(
            announcement,
            "Comments are disabled or unavailable for this video."
        );
        assert_eq!(view.labels, [announcement]);
        assert_eq!(state.request_load(&catalog), LoadRequest::NoMore);
        let mut state = CommentsState::default();
        let _ = state.request_load(&catalog);
        assert_eq!(
            state.finish_load(&catalog, Err("boom".to_owned())),
            LoadFinished::Failed {
                label: "Could not load comments: boom".to_owned(),
                announcement: "Could not load comments: boom".to_owned(),
            }
        );
        assert!(!state.loading());
    }

    #[test]
    fn worker_prefers_api_and_falls_back_like_python() {
        let catalog = embedded_catalog("en");
        let api_ok = |token: &str| -> Result<Value, String> {
            Ok(json!({"items": [{"snippet": {"topLevelComment": {"snippet":
                {"textOriginal": token, "authorDisplayName": "A"}}}}]}))
        };
        let ytdlp_unused = || -> Result<Value, String> { panic!("yt-dlp must not run") };
        let page = fetch_page(&catalog, "tok", Some(&api_ok), &ytdlp_unused).expect("page");
        assert_eq!(page.source_key, "comments_source_api");
        assert_eq!(page.comments[0].text, "tok");

        let api_fail =
            |_: &str| -> Result<Value, String> { Err("HTTP Error 403: Forbidden".to_owned()) };
        let ytdlp_ok =
            || -> Result<Value, String> { Ok(json!({"comments": [{"text": "from ytdlp"}]})) };
        let page = fetch_page(&catalog, "", Some(&api_fail), &ytdlp_ok).expect("fallback");
        assert_eq!(page.source_key, "comments_source_ytdlp");
        assert_eq!(page.comments[0].text, "from ytdlp");
        // A later page never falls back.
        assert_eq!(
            fetch_page(&catalog, "P2", Some(&api_fail), &ytdlp_unused),
            Err("HTTP Error 403: Forbidden".to_owned())
        );
        let ytdlp_fail =
            || -> Result<Value, String> { Err("ERROR: Sign in to confirm".to_owned()) };
        let error = fetch_page(&catalog, "", Some(&api_fail), &ytdlp_fail).unwrap_err();
        assert!(error.starts_with("HTTP Error 403: Forbidden\n\nERROR: Sign in to confirm\n\n"));
        assert!(error.ends_with(catalog.text("youtube_auth_hint")));
        let page = fetch_page(&catalog, "", None, &ytdlp_ok).expect("no key");
        assert_eq!(page.source_key, "comments_source_ytdlp");
    }

    #[test]
    fn source_url_prefers_the_stored_url_of_the_same_video() {
        let mut item = MediaItem::from_direct_link("https://youtu.be/dQw4w9WgXcQ?t=5").unwrap();
        assert_eq!(
            source_url(&item, "dQw4w9WgXcQ"),
            "https://youtu.be/dQw4w9WgXcQ?t=5"
        );
        assert_eq!(
            source_url(&item, "otherVideo1"),
            "https://www.youtube.com/watch?v=otherVideo1"
        );
        item.metadata.insert(
            "webpage_url".to_owned(),
            json!("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL1"),
        );
        assert_eq!(
            source_url(&item, "dQw4w9WgXcQ"),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL1"
        );
    }

    #[test]
    fn strip_html_matches_python_rules() {
        assert_eq!(strip_html("a<BR/>  b <i>c</i>\t\td"), "a\nb c d");
        assert_eq!(strip_html("  x\n   y  "), "x\ny");
    }
}
