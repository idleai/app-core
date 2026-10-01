use crux_core::Request;

use super::{context, ready, record, request, requests, resolve_load, send};
use crate::{
    Core, Effect, Event as RootEvent,
    configuration::{ConfigurationDocument as Document, ConfigurationLoadState, Event},
    subscriptions::{self, Context, SubscriptionOperation, SubscriptionResult},
    workspace::{
        self, RepositoryInfo, WorkspaceInfo, WorkspaceMode, WorkspaceOperation, WorkspaceResult,
    },
};

fn subscription_context() -> Context {
    let active = context(WorkspaceMode::Standalone);
    Context {
        provider: active.provider,
        workspace: active.workspace_id,
        contributor: active.contributor_id,
        chain: active.chain,
    }
}

fn subscription_request(effects: Vec<Effect>) -> Request<SubscriptionOperation> {
    effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Subscription(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("subscription effect")
}

#[test]
fn configuration_waits_for_buffered_join_and_cannot_override_subscription_audience() {
    let core = Core::new();
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(subscription_context()),
    )));
    assert!(
        requests(send(
            &core,
            Event::Connect(context(WorkspaceMode::Standalone))
        ))
        .is_empty(),
        "configuration waits for buffered join"
    );
    assert_eq!(
        core.view().configuration.settings.load,
        ConfigurationLoadState::Suspended,
        "waiting is visible"
    );
    assert!(
        requests(send(&core, Event::Reconnect)).is_empty(),
        "direct reconnect cannot bypass pending join"
    );
    let mut load = request(
        core.resolve(
            &mut join,
            Ok(SubscriptionResult::Joined {
                connection: "joined".into(),
            }),
        )
        .expect("joined"),
        Document::Settings,
    );
    let _effects = resolve_load(&core, &mut load, Some(record(3, "{}")));
    let mut other = context(WorkspaceMode::Standalone);
    other.contributor_id = "other".into();
    assert!(
        send(&core, Event::Connect(other)).is_empty(),
        "configuration cannot override connected audience"
    );
    assert_eq!(
        core.view().configuration.settings.load,
        ConfigurationLoadState::Ready,
        "join resumes configuration load"
    );
}

#[test]
fn subscription_audience_switch_clears_configuration_and_retires_outstanding_results() {
    let core = ready(WorkspaceMode::Standalone);
    let mut load = request(send(&core, Event::Refresh), Document::Settings);
    let mut other = subscription_context();
    other.contributor = "other".into();
    let _effects = core.process_event(RootEvent::Subscriptions(subscriptions::Event::Connect(
        other,
    )));
    assert!(
        core.view().configuration.context.is_none(),
        "audience switch clears both editors"
    );
    let _effects = resolve_load(&core, &mut load, Some(record(4, r#"{"private":true}"#)));
    assert!(
        core.view().configuration.settings.current.is_none(),
        "old response cannot expose previous audience's data"
    );
}

#[test]
fn workspace_selection_disconnects_configuration_and_guards_mode_chain_and_scope() {
    let core = ready(WorkspaceMode::Standalone);
    let effects = core.process_event(RootEvent::Workspace(workspace::Event::Load));
    let mut directory: Request<WorkspaceOperation> = effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Workspace(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("directory load");
    let _effects = core
        .resolve(
            &mut directory,
            Ok(WorkspaceResult::Directory(vec![WorkspaceInfo {
                id: "workspace".into(),
                name: "Workspace".into(),
                chain: "chain".into(),
                revision: 1,
                mode: WorkspaceMode::Standalone,
                repositories: vec![RepositoryInfo {
                    id: "repo".into(),
                    name: "Repo".into(),
                    remote: None,
                }],
            }])),
        )
        .expect("directory result");
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
        "workspace".into(),
    )));
    assert!(
        core.view().configuration.context.is_none(),
        "workspace ownership change clears configuration scope"
    );
    for bad in [
        crate::configuration::ConfigurationContext {
            mode: WorkspaceMode::Managed,
            ..context(WorkspaceMode::Standalone)
        },
        crate::configuration::ConfigurationContext {
            chain: "other".into(),
            ..context(WorkspaceMode::Standalone)
        },
        crate::configuration::ConfigurationContext {
            workspace_id: "other".into(),
            ..context(WorkspaceMode::Standalone)
        },
    ] {
        assert!(
            send(&core, Event::Connect(bad)).is_empty(),
            "configuration must match selected workspace binding"
        );
    }
}

#[test]
fn subscription_invalidation_and_transport_loss_preserve_unsaved_work() {
    let core = Core::new();
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(subscription_context()),
    )));
    let _effects = send(&core, Event::Connect(context(WorkspaceMode::Standalone)));
    let effects = core
        .resolve(
            &mut join,
            Ok(SubscriptionResult::Joined {
                connection: "joined".into(),
            }),
        )
        .expect("joined");
    let mut watch = None;
    for effect in effects {
        if let Effect::Configuration(mut load) = effect {
            let _effects = resolve_load(&core, &mut load, Some(record(3, "{}")));
        } else if let Effect::Subscription(request) = effect {
            watch = Some(*request);
        }
    }
    let mut watch = watch.expect("buffered watch");
    super::edit(&core, Document::Settings, r#"{"draft":true}"#);
    let mut write = super::save(&core, Document::Settings, "in-flight");
    let effects = core
        .resolve(&mut watch, Ok(SubscriptionResult::Changed))
        .expect("configuration notification");
    let mut next = None;
    let mut loaded_rules = false;
    for effect in effects {
        if let Effect::Configuration(load) = effect {
            assert_eq!(
                load.operation.document,
                Document::AgentRules,
                "settings refresh waits for the save outcome"
            );
            loaded_rules = true;
        } else if let Effect::Subscription(request) = effect {
            next = Some(*request);
        }
    }
    assert!(
        loaded_rules,
        "subscription invalidation refreshes configuration"
    );
    let _effects = core
        .resolve(
            &mut next.expect("next watch"),
            Ok(SubscriptionResult::Closed),
        )
        .expect("transport loss");
    let _effects = super::commit(&core, &mut write, 4);
    let view = core.view().configuration.settings;
    assert_eq!(
        view.load,
        ConfigurationLoadState::Suspended,
        "transport loss suspends reads"
    );
    assert!(
        matches!(
            view.save,
            crate::configuration::ConfigurationSaveState::Uncertain(_)
        ),
        "late save reply cannot bypass suspension"
    );
    assert_eq!(
        view.draft.json, r#"{"draft":true}"#,
        "connection loss preserves the draft"
    );
}
