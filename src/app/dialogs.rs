//! Dialog identities and the network-stream form state.

use enzui_core::form::{FieldId, FormField, FormModel, FormSubmission};
use enzui_widgets::form_dialog::FormPresenter;

pub(crate) const URL: &str = "url";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialog {
    OpenUrl,
    Info,
    Shortcuts,
}

impl Dialog {
    pub(crate) const fn close_button_id(self) -> iced::advanced::widget::Id {
        iced::advanced::widget::Id::new(match self {
            Self::OpenUrl => "video-url-close",
            Self::Info => "video-info-close",
            Self::Shortcuts => "video-shortcuts-close",
        })
    }
}

const URL_SCOPE: &str = "video-url";

fn url_model() -> FormModel {
    FormModel::new(vec![
        FormField::text(URL, enzui_core::localization::text("video-address-56ef8f"))
            .placeholder("https://example.com/video.mp4")
            .required(true),
    ])
    .expect("one field")
}

pub(super) fn open_url() -> FormPresenter {
    FormPresenter::new(url_model()).with_field_id_scope(URL_SCOPE)
}

pub(super) fn url_refused(submission: FormSubmission, reason: String) -> FormPresenter {
    FormPresenter::refused(url_model(), submission, &field(URL), reason)
        .with_field_id_scope(URL_SCOPE)
}

pub(crate) fn field(id: &str) -> FieldId {
    FieldId::new(id)
}
