use super::{App, Message, io, open};
use crate::model::Entry;
use enzui_widgets::file_drop::{DragAction, FileDrop};
use iced::Task;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct NativeDrop {
    next: u64,
    receiving: Option<u64>,
    retained: Vec<enzui_core::drop_files::CopiedFiles>,
}

#[derive(Clone, Debug)]
pub(crate) struct Offer {
    token: u64,
    receipt: Arc<Mutex<Option<FileDrop>>>,
}

impl Offer {
    pub(crate) fn new(token: u64, drop: FileDrop) -> Self {
        Self {
            token,
            receipt: Arc::new(Mutex::new(Some(drop))),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Delivery {
    id: u64,
    token: u64,
    owned: Option<enzui_core::drop_files::CopiedFiles>,
    videos: Vec<Entry>,
    subtitle: Option<PathBuf>,
    receipt: Arc<Mutex<Option<FileDrop>>>,
}

impl App {
    pub(crate) fn is_temporary_media(&self, path: &std::path::Path) -> bool {
        self.native_drop
            .retained
            .iter()
            .any(|owner| owner.contains(path))
    }

    pub(super) fn native_drop_allowed(&self) -> bool {
        self.dialogs.is_empty() && self.closing.is_none() && !self.chooser_pending()
    }

    pub(crate) fn drop_target_key(&self) -> (u64, bool, bool) {
        (
            self.open_token,
            self.native_drop_allowed(),
            self.native_drop.receiving.is_some(),
        )
    }

    pub(crate) fn receive_files(&mut self, offer: Offer) -> Task<Message> {
        let Some(receipt) = offer
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return Task::none();
        };
        if offer.token != self.open_token
            || !self.native_drop_allowed()
            || self.native_drop.receiving.is_some()
            || receipt.action() == Some(DragAction::Move)
        {
            return Task::none();
        }
        self.native_drop.next = self.native_drop.next.wrapping_add(1);
        let id = self.native_drop.next;
        self.native_drop.receiving = Some(id);
        let paths = receipt.paths().to_vec();
        let temporary = receipt.is_temporary();
        let budget = (2u64 * 1024 * 1024 * 1024).saturating_sub(
            self.native_drop
                .retained
                .iter()
                .map(enzui_core::drop_files::CopiedFiles::bytes)
                .sum(),
        );
        crate::l10n::perform(
            async move {
                let (videos, subtitle, owned) = io::disk(move || {
                    let owned = if temporary {
                        match enzui_core::drop_files::copy_files(
                            &paths,
                            None,
                            budget,
                            &enzui_core::cancellation::CancelToken::new(),
                        ) {
                            Ok(files) => Some(files),
                            Err(_) => return (Vec::new(), None, None),
                        }
                    } else {
                        None
                    };
                    let resolved = owned.as_ref().map_or(paths, |owner| owner.paths().to_vec());
                    let (videos, subtitle) = open::resolve(resolved);
                    (videos, subtitle, owned)
                })
                .await;
                Delivery {
                    id,
                    token: offer.token,
                    videos,
                    subtitle,
                    owned,
                    receipt: Arc::new(Mutex::new(Some(receipt))),
                }
            },
            Message::NativeFiles,
        )
    }

    pub(crate) fn native_files(&mut self, delivery: Delivery) -> Task<Message> {
        if self.native_drop.receiving != Some(delivery.id) {
            return Task::none();
        }
        let Some(receipt) = delivery
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return Task::none();
        };
        self.native_drop.receiving = None;
        let accepted =
            !delivery.videos.is_empty() || delivery.subtitle.is_some() && self.status.has_source();
        let Some(task) =
            self.stage_native_files(delivery.token, delivery.videos, delivery.subtitle)
        else {
            return Task::none();
        };
        if accepted {
            if let Some(owned) = delivery.owned {
                self.native_drop.retained.push(owned);
            }
            receipt.finish();
        }
        task
    }

    pub(super) fn stage_native_files(
        &mut self,
        token: u64,
        videos: Vec<Entry>,
        subtitle: Option<PathBuf>,
    ) -> Option<Task<Message>> {
        if token != self.open_token || !self.native_drop_allowed() {
            return None;
        }
        if videos.is_empty() && subtitle.is_some() && self.status.remote {
            self.toast_error(enzui_core::localization::text(
                "video-remote-external-subtitle",
            ));
            return None;
        }
        self.open_token = self.open_token.wrapping_add(1);
        self.drop_buffer.clear();
        self.last_drop = None;
        Some(self.drop_resolved(videos, subtitle, false))
    }
}
