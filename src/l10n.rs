use enzui_core::localization::{Catalog, CatalogBuilder, Localizer, preferred_locales};

fn catalog() -> Catalog {
    let mut catalog = CatalogBuilder::new();
    catalog
        .add_resource("en-US", include_str!("../assets/locales/en-US.ftl"))
        .expect("valid embedded English catalog");
    catalog
        .add_resource("ar", include_str!("../assets/locales/ar.ftl"))
        .expect("valid embedded Arabic catalog");
    catalog
        .add_resource("fr", include_str!("../assets/locales/fr.ftl"))
        .expect("valid embedded French catalog");
    catalog.build()
}

pub(crate) fn localizer() -> Localizer {
    Localizer::new(
        catalog(),
        preferred_locales(),
        "en-US".parse().expect("valid locale"),
    )
}

// Worker futures do not inherit the UI thread's app catalog.
pub(crate) fn perform<F, T, M>(
    future: F,
    map: impl FnOnce(T) -> M + Send + 'static,
) -> iced::Task<M>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
    M: Send + 'static,
{
    let localizer = enzui_core::localization::current();
    iced::Task::perform(
        async move {
            let mut future = std::pin::pin!(future);
            let value =
                std::future::poll_fn(|cx| localizer.scope(|| future.as_mut().poll(cx))).await;
            localizer.scope(|| map(value))
        },
        std::convert::identity,
    )
}

pub(crate) fn number(value: f64, fraction: usize) -> String {
    decimal(&format!("{value:.fraction$}"))
}

pub(crate) fn decimal(ascii: &str) -> String {
    ascii
        .parse::<enzui_core::localization::Decimal>()
        .ok()
        .and_then(|value| {
            enzui_core::localization::current()
                .locale()
                .format_decimal(&value)
                .ok()
        })
        .unwrap_or_else(|| ascii.to_owned())
}

pub(crate) fn rtl() -> bool {
    enzui_core::localization::current().locale().direction()
        == enzui_core::placement::Direction::RightToLeft
}

pub(crate) fn error(message: &str) -> String {
    match message {
        "video-session-expired" | "video-incorrect-pin" | "video-incorrect-password" => {
            enzui_core::localization::text(message)
        }
        _ => message.to_owned(),
    }
}

pub(crate) fn subtitle_size(size: enzui_services::media::SubtitleSize) -> String {
    use enzui_services::media::SubtitleSize;
    enzui_core::localization::text(match size {
        SubtitleSize::Tiny => "video-subtitle-tiny",
        SubtitleSize::ExtraSmall => "video-subtitle-extra-small",
        SubtitleSize::Small => "video-subtitle-small",
        SubtitleSize::Medium => "video-subtitle-medium",
        SubtitleSize::Large => "video-subtitle-large",
        SubtitleSize::ExtraLarge => "video-subtitle-extra-large",
    })
}

pub(crate) fn track(track: &enzui_services::media::Track) -> String {
    if track.language.is_none() && track.title.is_none() {
        enzui_core::localized!("video-track", "number" => track.index + 1)
    } else {
        track.label()
    }
}
