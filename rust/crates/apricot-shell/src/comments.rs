//! The comments window (Windows `show_player_comments` with
//! `comments_win32.rs`): the search field, the sort choice, the comment list
//! and Open, Copy, Copy visible, Author channel, Load more and Back. Pages
//! load on workers from the `YouTube` Data API (with a key) or yt-dlp; the
//! list logic is `apricot_app::comments::CommentsState`.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use apricot_app::comments::{
    ApiRequest, CommentSort, CommentsListView, CommentsPage, CommentsState, LoadFinished,
    LoadRequest, comment_copy_text, comment_details_text, comments_copy_text,
};
use apricot_core::TranslationCatalog;

use crate::{Shell, ShellUi};

/// Starts one page load on a worker (Windows `CommentsLoader`).
type CommentsLoader = Box<dyn FnMut(String) -> Receiver<Result<CommentsPage, String>>>;

/// A button or menu command of the window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommentsCommand {
    Open,
    Copy,
    CopyVisible,
    Author,
    More,
}

/// What a command does in the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommentsEffect {
    None,
    Announce(String),
    /// Python `show_comment_details`: a read-only text with Back.
    Details {
        title: String,
        back: String,
        text: String,
    },
    /// The list changes (a page started loading).
    Loading(String),
}

/// The comments window's state.
pub struct CommentsDialog {
    catalog: TranslationCatalog,
    state: CommentsState,
    loader: CommentsLoader,
    pending: Option<Receiver<Result<CommentsPage, String>>>,
}

impl CommentsDialog {
    /// A text of the window.
    #[must_use]
    pub fn text(&self, key: &str) -> String {
        self.catalog.text(key).to_owned()
    }

    /// The sort choices in order.
    #[must_use]
    pub fn sort_labels(&self) -> Vec<String> {
        CommentSort::ALL
            .iter()
            .map(|sort| self.text(sort.label_key()))
            .collect()
    }

    /// Python `refresh_comments`.
    pub fn refresh(&mut self, selection: usize) -> CommentsListView {
        self.state.refresh(&self.catalog, selection)
    }

    /// Windows `EN_CHANGE` of the search field.
    pub fn set_query(&mut self, query: &str) -> CommentsListView {
        self.state.set_query(query);
        self.refresh(0)
    }

    /// Windows `CBN_SELCHANGE` of the sort choice.
    pub fn set_sort(&mut self, index: usize) -> CommentsListView {
        if let Some(sort) = CommentSort::ALL.get(index) {
            self.state.set_sort(*sort);
        }
        self.refresh(0)
    }

    /// Windows `LBN_SELCHANGE`: whether the author's channel can open.
    #[must_use]
    pub fn author_enabled(&self, row: Option<usize>) -> bool {
        row.and_then(|row| self.state.selected(row))
            .is_some_and(|comment| !comment.author_channel_url.trim().is_empty())
    }

    /// Whether Load more is enabled.
    #[must_use]
    pub fn more_enabled(&self) -> bool {
        self.state.more_enabled()
    }

    /// Whether visible comments exist (Open, Copy and Copy visible).
    #[must_use]
    pub fn has_comments(&self) -> bool {
        !self.state.visible_comments().is_empty()
    }

    /// Python `load_more`.
    pub fn load_more(&mut self) -> CommentsEffect {
        match self.state.request_load(&self.catalog) {
            LoadRequest::Ignored => CommentsEffect::None,
            LoadRequest::NoMore => CommentsEffect::Announce(self.text("no_more_comments")),
            LoadRequest::Start {
                page_token,
                loading_label,
            } => {
                self.pending = Some((self.loader)(page_token));
                loading_label.map_or(CommentsEffect::None, CommentsEffect::Loading)
            }
        }
    }

    /// Python `finish_load`: `Some` once a page arrived.
    pub fn poll(&mut self) -> Option<LoadFinished> {
        let receiver = self.pending.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(String::new()),
        };
        self.pending = None;
        Some(self.state.finish_load(&self.catalog, result))
    }

    /// Windows `command` for everything but the clipboard and the browser,
    /// which the shell does.
    fn effect(&mut self, command: CommentsCommand, row: Option<usize>) -> CommentsEffect {
        let selected = row.and_then(|row| self.state.selected(row)).cloned();
        match command {
            CommentsCommand::Open => {
                selected.map_or(CommentsEffect::None, |comment| CommentsEffect::Details {
                    title: self.text("comment_details"),
                    back: self.text("back"),
                    text: comment_details_text(&self.catalog, &comment),
                })
            }
            CommentsCommand::More => self.load_more(),
            _ => CommentsEffect::None,
        }
    }
}

impl Shell {
    /// Windows `show_player_comments`: only `YouTube` videos have comments.
    pub(crate) fn show_player_comments(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if !self.ensure_player_for_auxiliary_view(ui) {
            return;
        }
        let Some(item) = self.application.player_session().current_item().cloned() else {
            return;
        };
        let catalog = self.catalog();
        let Some(video_id) = item.youtube_video_id() else {
            ui.set_status(catalog.text("comments_disabled"), true);
            return;
        };
        let source_url = apricot_app::comments::source_url(&item, &video_id);
        let settings = self.application.settings();
        let api_key = settings.youtube_data_api_key.trim().to_owned();
        let proxy = Some(settings.proxy.trim().to_owned()).filter(|proxy| !proxy.is_empty());
        let config = self.youtube_session_config();
        let executable = crate::youtube::components_directory()
            .map(|path| apricot_platform::app_update::preferred_ytdlp_executable(&path));
        let worker_catalog = catalog.clone();
        let loader: CommentsLoader = Box::new(move |page_token: String| {
            let (sender, receiver) = mpsc::channel();
            let catalog = worker_catalog.clone();
            let video_id = video_id.clone();
            let source_url = source_url.clone();
            let api_key = api_key.clone();
            let proxy = proxy.clone();
            let config = config.clone();
            let executable = executable.clone();
            std::thread::spawn(move || {
                let api = |token: &str| -> Result<serde_json::Value, String> {
                    use apricot_platform::YoutubeDataApiError as E;
                    apricot_platform::YoutubeDataApiClient::new(proxy.as_deref())
                        .and_then(|client| client.fetch_comment_threads(&api_key, &video_id, token))
                        .map_err(|error| match error {
                            E::CommentsDisabled => catalog.text("comments_disabled").to_owned(),
                            E::Api(message) | E::Http(message) | E::Transport(message) => message,
                            other => other.to_string(),
                        })
                };
                let ytdlp = || -> Result<serde_json::Value, String> {
                    use apricot_media::{YoutubeCommand, YoutubeEngine};
                    let path = executable.as_deref().unwrap_or(std::path::Path::new(""));
                    let mut engine = apricot_platform::YtDlpYoutubeEngine::new(path)
                        .map_err(|_| catalog.text("missing_ytdlp").to_owned())?;
                    engine
                        .execute(YoutubeCommand::Configure {
                            config: config.clone(),
                        })
                        .map_err(|error| error.to_string())?;
                    engine
                        .comments_metadata(&source_url)
                        .map_err(|error| match error {
                            apricot_platform::ytdlp_youtube::YtDlpError::Request(message) => {
                                message
                            }
                            other => other.to_string(),
                        })
                };
                let api: Option<&ApiRequest<'_>> = (!api_key.is_empty()).then_some(&api);
                let result = apricot_app::comments::fetch_page(&catalog, &page_token, api, &ytdlp);
                let _ = sender.send(result);
            });
            receiver
        });
        ui.open_comments(CommentsDialog {
            catalog,
            state: CommentsState::default(),
            loader,
            pending: None,
        });
    }

    /// A command of the comments window: the clipboard and the browser are
    /// used here, the rest is the window's.
    pub fn comments_command(
        &mut self,
        ui: &mut dyn ShellUi,
        dialog: &mut CommentsDialog,
        command: CommentsCommand,
        row: Option<usize>,
    ) -> CommentsEffect {
        let selected = row.and_then(|row| dialog.state.selected(row)).cloned();
        match command {
            CommentsCommand::Copy => {
                let Some(comment) = selected else {
                    return CommentsEffect::Announce(dialog.text(dialog.state.empty_message_key()));
                };
                let text = comment_copy_text(&dialog.catalog, &comment, None);
                if ui.copy_text(&text) {
                    CommentsEffect::Announce(dialog.text("comment_copied"))
                } else {
                    CommentsEffect::None
                }
            }
            CommentsCommand::CopyVisible => {
                let visible = dialog.state.visible_comments();
                if visible.is_empty() {
                    return CommentsEffect::Announce(dialog.text(dialog.state.empty_message_key()));
                }
                let text = comments_copy_text(&dialog.catalog, &visible);
                let count = visible.len();
                if ui.copy_text(&text) {
                    CommentsEffect::Announce(
                        dialog
                            .text("comments_copied")
                            .replace("{count}", &count.to_string()),
                    )
                } else {
                    CommentsEffect::None
                }
            }
            CommentsCommand::Author => {
                let url = selected
                    .map(|comment| comment.author_channel_url.trim().to_owned())
                    .unwrap_or_default();
                // Python `open_remote_url_in_browser(url, announce_error=False)`.
                let opened = url::Url::parse(&url).is_ok_and(|parsed| {
                    matches!(parsed.scheme(), "http" | "https")
                        && parsed.host_str().is_some()
                        && apricot_platform::shell::open_url(parsed.as_str()).is_ok()
                });
                CommentsEffect::Announce(dialog.text(if opened {
                    "comment_author_channel_opened"
                } else {
                    "comment_author_channel_unavailable"
                }))
            }
            CommentsCommand::Open | CommentsCommand::More => dialog.effect(command, row),
        }
    }

    /// The comments window closed: focus goes to the player (Python
    /// `focus_player_target_later("player")`).
    pub fn comments_closed(&mut self, ui: &mut dyn ShellUi) {
        ui.focus_primary_control();
    }
}
