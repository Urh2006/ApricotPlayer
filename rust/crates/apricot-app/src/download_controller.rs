//! Application-owned transient download queue and task state.

use apricot_core::MediaItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadChoice {
    Ask,
    Audio,
    Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadTaskKind {
    Single,
    Playlist,
    Channel,
    PodcastFeed,
    UserPlaylist,
    Batch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadTaskStatus {
    Downloading,
    Processing,
    CancelRequested,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueuedDownload {
    pub item: MediaItem,
    pub choice: DownloadChoice,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveDownload {
    pub id: u64,
    pub item: MediaItem,
    pub choice: DownloadChoice,
    pub kind: DownloadTaskKind,
    pub status: DownloadTaskStatus,
    pub title: String,
    pub current_title: String,
    pub percent: Option<f64>,
    pub total: usize,
    pub completed: usize,
    pub item_failures: Vec<String>,
}

impl ActiveDownload {
    pub fn remaining(&self) -> usize {
        self.total.saturating_sub(self.completed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueToggleOutcome {
    Selected,
    SelectionChanged,
    Deselected,
    Rejected,
}

#[derive(Debug, Default)]
pub struct DownloadController {
    queued: Vec<QueuedDownload>,
    active: Vec<ActiveDownload>,
    next_id: u64,
}

impl DownloadController {
    pub fn count(&self) -> usize {
        self.active.len().saturating_add(self.queued.len())
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty() && self.queued.is_empty()
    }

    pub fn queued(&self) -> &[QueuedDownload] {
        &self.queued
    }

    pub fn active(&self) -> &[ActiveDownload] {
        &self.active
    }

    pub fn active_task(&self, id: u64) -> Option<&ActiveDownload> {
        self.active.iter().find(|task| task.id == id)
    }

    pub fn queue_item(&mut self, item: MediaItem, choice: DownloadChoice) -> QueueToggleOutcome {
        let Some(identity) = item.stable_identity() else {
            return QueueToggleOutcome::Rejected;
        };
        if let Some(index) = self
            .queued
            .iter()
            .position(|queued| queued.item.stable_identity().as_deref() == Some(identity.as_str()))
        {
            if self.queued[index].choice == choice {
                self.queued.remove(index);
                return QueueToggleOutcome::Deselected;
            }
            self.queued[index] = QueuedDownload { item, choice };
            return QueueToggleOutcome::SelectionChanged;
        }
        self.queued.push(QueuedDownload { item, choice });
        QueueToggleOutcome::Selected
    }

    pub fn remove_queued(&mut self, item: &MediaItem) -> Option<QueuedDownload> {
        let identity = item.stable_identity()?;
        let index = self.queued.iter().position(|queued| {
            queued.item.stable_identity().as_deref() == Some(identity.as_str())
        })?;
        Some(self.queued.remove(index))
    }

    pub fn take_queued(&mut self) -> Vec<QueuedDownload> {
        std::mem::take(&mut self.queued)
    }

    pub fn clear_queued(&mut self) -> usize {
        let count = self.queued.len();
        self.queued.clear();
        count
    }

    pub fn begin(
        &mut self,
        item: MediaItem,
        choice: DownloadChoice,
        kind: DownloadTaskKind,
        total: usize,
    ) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        let title = item.title.clone();
        self.active.push(ActiveDownload {
            id,
            item,
            choice,
            kind,
            status: DownloadTaskStatus::Downloading,
            title: title.clone(),
            current_title: title,
            percent: None,
            total,
            completed: 0,
            item_failures: Vec::new(),
        });
        id
    }

    pub fn update_progress(
        &mut self,
        id: u64,
        status: DownloadTaskStatus,
        current_title: &str,
        percent: Option<f64>,
        playlist_index: Option<usize>,
        playlist_count: Option<usize>,
    ) -> bool {
        let Some(task) = self.active.iter_mut().find(|task| task.id == id) else {
            return false;
        };
        if task.status == DownloadTaskStatus::CancelRequested {
            return false;
        }
        task.status = status;
        if !current_title.trim().is_empty() {
            current_title.clone_into(&mut task.current_title);
        }
        task.percent = percent
            .filter(|value| value.is_finite())
            .map(|value| value.clamp(0.0, 100.0));
        if let Some(total) = playlist_count.filter(|total| *total > 0) {
            task.total = total;
        }
        if let Some(index) = playlist_index.filter(|index| *index > 0) {
            let current_completed = if status == DownloadTaskStatus::Processing {
                index
            } else {
                index.saturating_sub(1)
            };
            task.completed = task.completed.max(current_completed.min(task.total));
        }
        true
    }

    pub fn set_completed(&mut self, id: u64, completed: usize, total: usize) -> bool {
        let Some(task) = self.active.iter_mut().find(|task| task.id == id) else {
            return false;
        };
        task.total = total;
        task.completed = completed.min(total);
        true
    }

    pub fn add_item_failure(&mut self, id: u64, message: &str) -> bool {
        let Some(task) = self.active.iter_mut().find(|task| task.id == id) else {
            return false;
        };
        let message = message.trim();
        if message.is_empty() || task.item_failures.iter().any(|value| value == message) {
            return false;
        }
        task.item_failures.push(message.to_owned());
        true
    }

    pub fn request_cancel(&mut self, id: u64) -> bool {
        let Some(task) = self.active.iter_mut().find(|task| task.id == id) else {
            return false;
        };
        task.status = DownloadTaskStatus::CancelRequested;
        true
    }

    pub fn request_cancel_all(&mut self) -> Vec<u64> {
        self.active
            .iter_mut()
            .map(|task| {
                task.status = DownloadTaskStatus::CancelRequested;
                task.id
            })
            .collect()
    }

    pub fn finish(&mut self, id: u64) -> Option<ActiveDownload> {
        let index = self.active.iter().position(|task| task.id == id)?;
        Some(self.active.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use apricot_core::MediaItem;

    use super::{
        DownloadChoice, DownloadController, DownloadTaskKind, DownloadTaskStatus,
        QueueToggleOutcome,
    };

    fn item(url: &str, title: &str) -> MediaItem {
        MediaItem::from_direct_link(url)
            .map(|mut item| {
                item.title = title.to_owned();
                item
            })
            .expect("item")
    }

    #[test]
    fn same_choice_toggles_off_and_different_choice_replaces_in_place() {
        let mut controller = DownloadController::default();
        let media = item("https://example.com/one", "One");
        assert_eq!(
            controller.queue_item(media.clone(), DownloadChoice::Audio),
            QueueToggleOutcome::Selected
        );
        assert_eq!(
            controller.queue_item(media.clone(), DownloadChoice::Video),
            QueueToggleOutcome::SelectionChanged
        );
        assert_eq!(controller.queued().len(), 1);
        assert_eq!(controller.queued()[0].choice, DownloadChoice::Video);
        assert_eq!(
            controller.queue_item(media, DownloadChoice::Video),
            QueueToggleOutcome::Deselected
        );
        assert!(controller.is_empty());
    }

    #[test]
    fn active_tasks_and_queue_share_one_menu_count_with_monotonic_ids() {
        let mut controller = DownloadController::default();
        controller.queue_item(
            item("https://example.com/queued", "Queued"),
            DownloadChoice::Ask,
        );
        let first = controller.begin(
            item("https://example.com/one", "One"),
            DownloadChoice::Audio,
            DownloadTaskKind::Single,
            1,
        );
        let second = controller.begin(
            item("https://example.com/two", "Two"),
            DownloadChoice::Video,
            DownloadTaskKind::Single,
            1,
        );
        assert!(second > first);
        assert_eq!(controller.count(), 3);
        assert_eq!(controller.active()[0].id, first);
    }

    #[test]
    fn playlist_progress_uses_python_completed_item_semantics() {
        let mut controller = DownloadController::default();
        let id = controller.begin(
            item("https://example.com/list", "List"),
            DownloadChoice::Audio,
            DownloadTaskKind::Playlist,
            0,
        );
        assert!(controller.update_progress(
            id,
            DownloadTaskStatus::Downloading,
            "Episode two",
            Some(42.5),
            Some(2),
            Some(10),
        ));
        let task = controller.active_task(id).expect("task");
        assert_eq!(task.total, 10);
        assert_eq!(task.completed, 1);
        assert_eq!(task.remaining(), 9);
        assert!(controller.update_progress(
            id,
            DownloadTaskStatus::Processing,
            "Episode two",
            Some(100.0),
            Some(2),
            Some(10),
        ));
        assert_eq!(controller.active_task(id).expect("task").completed, 2);
    }

    #[test]
    fn cancellation_blocks_late_progress_and_finish_removes_only_target() {
        let mut controller = DownloadController::default();
        let first = controller.begin(
            item("https://example.com/one", "One"),
            DownloadChoice::Audio,
            DownloadTaskKind::Single,
            1,
        );
        let second = controller.begin(
            item("https://example.com/two", "Two"),
            DownloadChoice::Audio,
            DownloadTaskKind::Batch,
            2,
        );
        assert!(controller.request_cancel(first));
        assert!(!controller.update_progress(
            first,
            DownloadTaskStatus::Downloading,
            "late",
            Some(90.0),
            None,
            None,
        ));
        assert_eq!(
            controller.active_task(first).expect("first").status,
            DownloadTaskStatus::CancelRequested
        );
        assert_eq!(controller.finish(first).expect("finished").id, first);
        assert_eq!(controller.active()[0].id, second);
    }

    #[test]
    fn batch_failures_are_deduplicated_and_do_not_finish_task() {
        let mut controller = DownloadController::default();
        let id = controller.begin(
            item("https://example.com/list", "List"),
            DownloadChoice::Audio,
            DownloadTaskKind::Batch,
            3,
        );
        assert!(controller.add_item_failure(id, "Unavailable"));
        assert!(!controller.add_item_failure(id, "Unavailable"));
        assert_eq!(
            controller
                .active_task(id)
                .expect("task")
                .item_failures
                .len(),
            1
        );
        assert_eq!(controller.active().len(), 1);
    }
}
