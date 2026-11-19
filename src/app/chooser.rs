use super::{App, Message, Pick};
use enzui_core::portal::{CancelReason, PortalOutcome, cancel::PortalCancel};
use enzui_widgets::file_chooser::{ChooserOptions, ChooserParent, FileResult, PortalOperation};
use enzui_widgets::portal_parent::{self, ParentExport};
use iced::Task;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
pub(super) struct Parenting {
    generation: u64,
    pending: Option<Request>,
    exports: Option<enzui_widgets::export_pool::ExportPoolOwner>,
}

struct Request {
    generation: u64,
    pick: Pick,
    operation: PortalOperation,
    options: ChooserOptions,
    cancel: PortalCancel,
}

impl Drop for Parenting {
    fn drop(&mut self) {
        if let Some(request) = self.pending.take() {
            request.cancel.cancel(CancelReason::Application);
        }
    }
}

type ParentResult = Result<ChooserParent, PortalOutcome<FileResult>>;

#[derive(Clone, Debug)]
pub(crate) struct ParentReply(Arc<Mutex<Option<ParentResult>>>);

impl ParentReply {
    fn new(parent: ParentResult) -> Self {
        Self(Arc::new(Mutex::new(Some(parent))))
    }
}

impl App {
    pub(crate) fn chooser_pending(&self) -> bool {
        self.chooser_parent.pending.is_some() || self.chooser.is_pending()
    }

    pub(super) fn start_chooser(
        &mut self,
        pick: Pick,
        operation: PortalOperation,
        options: ChooserOptions,
    ) -> Task<Message> {
        if self.closing.is_some() || self.chooser_pending() {
            return Task::none();
        }
        self.chooser_parent.generation = self.chooser_parent.generation.wrapping_add(1);
        let generation = self.chooser_parent.generation;
        self.chooser_parent.pending = Some(Request {
            generation,
            pick,
            operation,
            options,
            cancel: PortalCancel::new(),
        });
        iced::window::latest().map(move |window| Message::ChooserWindow(generation, window))
    }

    pub(super) fn chooser_window(
        &mut self,
        generation: u64,
        window: Option<iced::window::Id>,
    ) -> Task<Message> {
        let Some(request) = self
            .chooser_parent
            .pending
            .as_ref()
            .filter(|request| request.generation == generation)
        else {
            return Task::none();
        };
        let Some(window) = window else {
            return Task::done(Message::ChooserParent(
                generation,
                ParentReply::new(Err(PortalOutcome::ParentUnavailable)),
            ));
        };
        let cancel = request.cancel.clone();
        let pool = self
            .chooser_parent
            .exports
            .get_or_insert_with(|| {
                enzui_widgets::export_pool::ExportPoolOwner::start()
                    .unwrap_or_else(|_| enzui_widgets::export_pool::ExportPoolOwner::unavailable())
            })
            .dispatcher();
        iced::window::run(window, |window| {
            window
                .window_handle()
                .ok()
                .map_or(ParentExport::Unsupported, |handle| {
                    portal_parent::export_parent(&handle.as_raw())
                })
        })
        .then(move |export| match export {
            ParentExport::Ready(handle) => Task::done(Message::ChooserParent(
                generation,
                ParentReply::new(Ok(ChooserParent::X11 { window, handle })),
            )),
            ParentExport::NeedsWaylandExport => portal_parent::export_parent_task(
                &pool,
                generation,
                window,
                cancel.clone(),
                Duration::from_secs(3),
            )
            .map(move |handoff| {
                let parent = handoff
                    .take()
                    .and_then(|settled| settled.into_result().ok())
                    .and_then(|adoption| match adoption {
                        enzui_wayland::ParentAdoption::Parented { export, .. } => {
                            Some(ChooserParent::Wayland { window, export })
                        }
                        _ => None,
                    })
                    .ok_or(PortalOutcome::ParentUnavailable);
                Message::ChooserParent(generation, ParentReply::new(parent))
            }),
            ParentExport::Unsupported => Task::done(Message::ChooserParent(
                generation,
                ParentReply::new(Err(PortalOutcome::ParentUnavailable)),
            )),
        })
    }

    pub(super) fn chooser_parent(&mut self, generation: u64, reply: ParentReply) -> Task<Message> {
        if !self
            .chooser_parent
            .pending
            .as_ref()
            .is_some_and(|request| request.generation == generation)
        {
            return Task::none();
        }
        let Some(parent) = reply
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        else {
            return Task::none();
        };
        let Some(request) = self.chooser_parent.pending.take() else {
            return Task::none();
        };
        if self.closing.is_some() {
            return Task::none();
        }
        request.cancel.complete();
        let outcome = parent.and_then(|parent| {
            self.chooser.start_parented(
                request.pick,
                request.operation,
                request.options,
                parent,
                |_| self.closing.is_none(),
            )
        });
        match outcome {
            Ok(()) => Task::none(),
            Err(outcome) => self.chosen(request.pick, outcome),
        }
    }

    pub(super) fn cancel_chooser(&mut self, reason: CancelReason) {
        if let Some(request) = self.chooser_parent.pending.take() {
            request.cancel.cancel(reason);
        }
        let _ = self.chooser.cancel(reason);
    }
}
