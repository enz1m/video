use crate::app::{App, Message};
use enzui_core::activation::dbus::{ApplicationService, ApplicationServiceError, ApplicationStart};
use enzui_core::activation::{ActivationBroker, ApplicationIntent, InboundActivationToken};
use enzui_widgets::single_instance::{InstanceStart, SingleInstance};
use iced::{Subscription, Task};
use std::ffi::OsString;
use std::time::Instant;

pub(crate) struct Instance {
    shared: SingleInstance,
    service: Option<ApplicationService>,
    broker: ActivationBroker,
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            shared: SingleInstance::standalone(),
            service: None,
            broker: ActivationBroker::default(),
        }
    }
}

pub(crate) fn connect(opening: &[OsString]) -> Result<Option<Instance>, String> {
    if !arguments(opening.to_vec()).1 {
        return Ok(match SingleInstance::start(crate::app::DESKTOP_APP_ID) {
            InstanceStart::Forwarded => None,
            InstanceStart::Owner(shared, _) => Some(Instance {
                shared,
                ..Instance::default()
            }),
        });
    }
    // Library flags require an action intent; the ordinary adapter only forwards files.
    let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
    let intent = launch_intent(opening, &cwd);
    let mut token = InboundActivationToken::from_env().map_err(|error| format!("{error:?}"))?;
    match ApplicationService::connect_or_forward_with_token(
        crate::app::DESKTOP_APP_ID,
        Some(&intent),
        &mut token,
    ) {
        Ok(ApplicationStart::Reused) => Ok(None),
        Ok(ApplicationStart::Owner(service)) => Ok(Some(Instance {
            shared: SingleInstance::standalone(),
            service: Some(service),
            broker: ActivationBroker::default(),
        })),
        Err(ApplicationServiceError::Connect(error)) => {
            eprintln!("Desktop bus unavailable: {error}");
            Ok(Some(Instance::default()))
        }
        Err(error) => Err(error.to_string()),
    }
}

fn launch_intent(opening: &[OsString], cwd: &std::path::Path) -> ApplicationIntent {
    let (files, browse, _) = arguments(opening.to_vec());
    if browse && files.is_empty() {
        ApplicationIntent::ActivateAction {
            name: "library".into(),
        }
    } else {
        enzui_core::activation::launch_request(
            std::iter::once(OsString::from("--")).chain(files),
            cwd,
        )
        .intent
    }
}

pub(crate) fn arguments(opening: Vec<OsString>) -> (Vec<OsString>, bool, Option<String>) {
    let mut args = opening.into_iter();
    let mut files = Vec::new();
    let mut browse = false;
    let mut server = None;
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional && arg == "--" {
            positional = true;
        } else if !positional && arg == "--plex" {
            browse = true;
        } else if !positional && arg == "--plex-server" {
            server = args.next().and_then(|value| value.into_string().ok());
            browse = true;
        } else {
            files.push(arg);
        }
    }
    (files, browse, server)
}

impl Instance {
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        self.service.as_ref().map_or_else(
            || self.shared.subscription(|_| Message::ApplicationWake),
            |service| {
                service
                    .wake_subscription(1)
                    .map(|_| Message::ApplicationWake)
            },
        )
    }
}

impl App {
    pub(crate) fn application_wake(&mut self) -> Task<Message> {
        let Some(service) = self.instance.service.as_mut() else {
            let (intents, raise) = self.instance.shared.drain(None);
            let mut tasks = vec![raise];
            for intent in intents {
                match intent {
                    ApplicationIntent::Open(uris) => tasks
                        .push(self.open_arguments(uris.into_iter().map(OsString::from).collect())),
                    ApplicationIntent::ActivateAction { name } if name == "library" => {
                        tasks.push(self.plex_event(crate::plex::Event::Open));
                    }
                    _ => {}
                }
            }
            return Task::batch(tasks);
        };
        let calls = service.drain(&mut self.instance.broker, Instant::now(), 16);
        let mut tasks = Vec::new();
        for call in calls {
            match call.intent {
                ApplicationIntent::Open(uris) => {
                    tasks.push(self.open_arguments(uris.into_iter().map(OsString::from).collect()));
                }
                ApplicationIntent::ActivateAction { name } if name == "library" => {
                    tasks.push(self.plex_event(crate::plex::Event::Open));
                }
                _ => {}
            }
            let mut context = call.context;
            tasks.push(iced::window::latest().then(move |window| {
                match (window, context.take()) {
                    (Some(window), Some(context)) => context
                        .activate(window, Instant::now())
                        .map(|_| Message::ApplicationWake),
                    (Some(window), None) => iced::window::gain_focus(window),
                    _ => Task::none(),
                }
            }));
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarding_encodes_literal_option_names_and_native_path_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let args = vec![
            OsString::from("--"),
            OsString::from("--plex"),
            OsString::from_vec(vec![b'-', 0xff]),
        ];
        assert_eq!(
            launch_intent(&args, std::path::Path::new("/tmp")),
            ApplicationIntent::Open(vec!["file:///tmp/--plex".into(), "file:///tmp/-%FF".into()])
        );
    }
}
