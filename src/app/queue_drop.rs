//! Queue metadata is scoped to this live queue; rearranging never restarts playback.
use super::{App, Message};
use crate::model::Entry;
use iced::Task;

pub(crate) const MIME: &str = "application/x-enzim-video-queue";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    owner: String,
    revision: u64,
    entry: String,
}

impl App {
    pub(crate) fn queue_drag_data(&self, entry: &Entry) -> Vec<u8> {
        serde_json::to_vec(&Row {
            owner: self.queue_drag_owner.clone(),
            revision: self.playlist.revision(),
            entry: crate::model::history::thumbnail_key(entry),
        })
        .unwrap_or_default()
    }

    pub(super) fn reorder_dropped(
        &mut self,
        target: Option<Entry>,
        drop: enzui_widgets::file_drop::DataDrop,
    ) -> Task<Message> {
        if !self.queue_available()
            || !self.native_drop_allowed()
            || drop
                .action()
                .is_some_and(|action| action != enzui_widgets::file_drop::DragAction::Copy)
            || drop.data().mime().as_str() != MIME
            || drop.data().bytes().len() > 1024
        {
            return Task::none();
        }
        let Ok(row) = serde_json::from_slice::<Row>(drop.data().bytes()) else {
            return Task::none();
        };
        if row.owner != self.queue_drag_owner
            || row.revision != self.playlist.revision()
            || target
                .as_ref()
                .is_some_and(|entry| !self.playlist.entries().contains(entry))
        {
            return Task::none();
        }
        let Some(entry) = self
            .playlist
            .entries()
            .iter()
            .find(|entry| crate::model::history::thumbnail_key(entry) == row.entry)
            .cloned()
        else {
            return Task::none();
        };
        if self.playlist.reorder_before(&entry, target.as_ref()) {
            self.refresh_menus();
        }
        drop.finish();
        Task::none()
    }
}
