//! Root composition of domain models, events and views.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use crate::{
    bootstrap, configuration, effects::Effect, history, projections, repository, resources,
    sessions, subscriptions, workspace,
};

/// State owned by one client, partitioned by domain reducer.
#[derive(Debug, Default)]
pub struct Model {
    initialized: bool,
    bootstrap: bootstrap::Model,
    history: history::Model,
    workspace: workspace::Model,
    subscriptions: subscriptions::Model,
    sessions: sessions::Model,
    projections: projections::Model,
    resources: resources::Model,
    configuration: configuration::Model,
    repository: repository::Model,
}

/// Client actions and domain events accepted by the application.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum Event {
    /// Initialize the client and load host information.
    Start,
    /// Route an action to the bootstrap domain.
    Bootstrap(bootstrap::Event),
    /// Route a semantic history action.
    History(history::Event),
    /// Route workspace selection, membership and presence actions.
    Workspace(workspace::Event),
    /// Route shared joins, reconnects and subscription lifetimes.
    Subscriptions(subscriptions::Event),
    /// Route owned/invited sessions, sharing and attributed prompts.
    Sessions(sessions::Event),
    /// Route projection selection, filtering, reads and history drill-down.
    Projections(projections::Event),
    /// Route compute/provider discovery, model actions and controller state.
    Resources(resources::Event),
    /// Route versioned settings and agent-rule editors.
    Configuration(configuration::Event),
    /// Route repository reads and recorded-session history selection.
    Repository(repository::Event),
}

/// The typed presentation state shared by all client surfaces.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct ViewModel {
    /// Whether the client has processed its start event (not host readiness).
    pub initialized: bool,
    /// Loading, error or host information from the bootstrap reducer.
    pub bootstrap: bootstrap::ViewModel,
    /// Shared history interaction, stored records and field content.
    pub history: history::ViewModel,
    /// Shared workspace/repository navigation, members and presence.
    pub workspace: workspace::ViewModel,
    /// Shared connection status and reconciliation readiness.
    pub subscriptions: subscriptions::SubscriptionViewModel,
    /// Session selection, pending mutations and runtime-confirmed input facts.
    pub sessions: sessions::ViewModel,
    /// Activity, task, error, triage and need-input views with source references.
    pub projections: projections::ViewModel,
    /// Resource availability, allowed model actions, progress and controller status.
    pub resources: resources::ViewModel,
    /// Versioned settings and rules, pending drafts and provider save feedback.
    pub configuration: configuration::ConfigurationViewModel,
    /// Git/GitHub data and recorded-session selection, without live runtime claims.
    pub repository: repository::ViewModel,
}

/// Root reducer composing the shared application's domain modules.
#[derive(Debug, Default)]
pub struct IdleApp;

impl App for IdleApp {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        let event = match event {
            Event::Start => {
                if model.initialized {
                    return Command::done();
                }
                model.initialized = true;
                // Starting after an explicit module load must still render this change.
                return bootstrap::Bootstrap
                    .update(bootstrap::Event::Load, &mut model.bootstrap)
                    .map_event(Event::Bootstrap)
                    .and(render::render());
            }
            Event::Bootstrap(event) => event,
            Event::History(event) => {
                // Workspace navigation owns the binding while connected. Direct
                // chain selection remains available to independent history clients.
                if model.subscriptions.context().is_some()
                    && matches!(
                        event,
                        history::Event::Connect(_) | history::Event::Disconnect
                    )
                {
                    return Command::done();
                }
                if model.workspace.owns_history()
                    && (matches!(&event, history::Event::Connect(chain) if model.workspace.chain() != Some(chain.as_str()))
                        || matches!(event, history::Event::Disconnect))
                {
                    return Command::done();
                }
                let command = history::History
                    .update(event, &mut model.history)
                    .map_event(Event::History);
                model
                    .subscriptions
                    .history_state(model.history.reconciliation());
                return command;
            }
            Event::Subscriptions(event) => return update_subscription(event, model),
            Event::Projections(event) => return update_projection(event, model),
            Event::Resources(event) => return update_resources(event, model),
            Event::Configuration(event) => return update_configuration(event, model),
            Event::Repository(event) => return update_repository(event, model),
            Event::Sessions(event) => {
                if let sessions::Event::Connect(context) = &event
                    && ((model.workspace.owns_history()
                        && (model.workspace.chain() != Some(context.chain.as_str())
                            || model.workspace.workspace_id()
                                != Some(context.workspace_id.as_str())
                            || model.workspace.coordination_mode() != Some(context.mode)))
                        || model.subscriptions.context().is_some_and(|active| {
                            active.workspace != context.workspace_id
                                || active.chain != context.chain
                                || active.contributor != context.contributor_id
                                || active.provider != context.provider
                        }))
                {
                    return Command::done();
                }
                return sessions::Sessions
                    .update(event, &mut model.sessions)
                    .map_event(Event::Sessions);
            }
            Event::Workspace(event) => {
                let navigation = if let workspace::Event::Navigate(section) = &event {
                    Some(*section)
                } else {
                    None
                };
                let before = (
                    model.workspace.chain().map(str::to_owned),
                    model.workspace.owns_history(),
                    model.workspace.workspace_id().map(str::to_owned),
                    model.workspace.coordination_mode(),
                );
                let command = workspace::Workspace
                    .update(event, &mut model.workspace)
                    .map_event(Event::Workspace);
                let after = (
                    model.workspace.chain().map(str::to_owned),
                    model.workspace.owns_history(),
                    model.workspace.workspace_id().map(str::to_owned),
                    model.workspace.coordination_mode(),
                );
                if before == after {
                    let binding = workspace::Workspace
                        .view(&model.workspace)
                        .repository_binding;
                    let retired = model.workspace.owns_history()
                        && model.repository.context().is_some_and(|context| {
                            !binding.as_ref().is_some_and(|binding| {
                                binding.workspace_id == context.connection.workspace
                                    && binding.chain == context.connection.chain
                                    && binding.repository_id == context.repository_id
                            })
                        });
                    let command = if retired {
                        command.and(update_repository(repository::Event::Disconnect, model))
                    } else {
                        command
                    };
                    return command.and(navigate_repository(navigation, model));
                }
                let command =
                    command.and(update_subscription(subscriptions::Event::Disconnect, model));
                let command = command.and(retire_domains(model, None));
                model.history.bind(None);
                let event = after
                    .0
                    .map_or(history::Event::Disconnect, history::Event::Connect);
                return command.and(
                    history::History
                        .update(event, &mut model.history)
                        .map_event(Event::History),
                );
            }
        };
        bootstrap::Bootstrap
            .update(event, &mut model.bootstrap)
            .map_event(Event::Bootstrap)
    }

    fn view(&self, model: &Model) -> ViewModel {
        ViewModel {
            initialized: model.initialized,
            bootstrap: bootstrap::Bootstrap.view(&model.bootstrap),
            history: history::History.view(&model.history),
            workspace: workspace::Workspace.view(&model.workspace),
            subscriptions: subscriptions::Subscriptions.view(&model.subscriptions),
            sessions: sessions::Sessions.view(&model.sessions),
            projections: projections::Projections.view(&model.projections),
            resources: resources::Resources.view(&model.resources),
            configuration: configuration::Configuration.view(&model.configuration),
            repository: repository::Repositories.view(&model.repository),
        }
    }
}

fn update_subscription(event: subscriptions::Event, model: &mut Model) -> Command<Effect, Event> {
    if let subscriptions::Event::Connect(context) = &event
        && model.workspace.owns_history()
        && (model.workspace.chain() != Some(context.chain.as_str())
            || model.workspace.workspace_id() != Some(context.workspace.as_str()))
    {
        return Command::done();
    }
    let before = model.subscriptions.context().cloned();
    let command = subscriptions::Subscriptions
        .update(event, &mut model.subscriptions)
        .map_event(Event::Subscriptions);
    let after = model.subscriptions.context().cloned();
    let command = if before == after {
        command
    } else {
        model
            .history
            .bind(after.as_ref().map(|context| context.chain.clone()));
        command.and(retire_domains(model, after.as_ref()))
    };
    if let Some(event) = model.subscriptions.take_history_event() {
        let repository_event = match &event {
            history::Event::Refresh => Some(repository::Event::Changed),
            history::Event::Reconnect => Some(repository::Event::Reconnect),
            history::Event::Suspend => Some(repository::Event::Suspend),
            history::Event::Disconnect => Some(repository::Event::Disconnect),
            history::Event::Connect(_)
            | history::Event::SetFilter(_)
            | history::Event::LoadMore
            | history::Event::Search(_)
            | history::Event::SearchMore
            | history::Event::NavigateMatch(_)
            | history::Event::Select(_)
            | history::Event::ClearSelection
            | history::Event::ToggleDisclosure(_)
            | history::Event::LoadItem(_)
            | history::Event::LoadOperationDetails { .. }
            | history::Event::Open { .. }
            | history::Event::Completed { .. } => None,
        };
        let command = if let Some(event) = repository_event {
            command.and(update_repository(event, model))
        } else {
            command
        };
        let configuration_event = match &event {
            history::Event::Refresh => Some(configuration::Event::Refresh),
            history::Event::Reconnect => Some(configuration::Event::Reconnect),
            history::Event::Suspend => Some(configuration::Event::Suspend),
            history::Event::Disconnect => Some(configuration::Event::Disconnect),
            history::Event::Connect(_)
            | history::Event::SetFilter(_)
            | history::Event::LoadMore
            | history::Event::Search(_)
            | history::Event::SearchMore
            | history::Event::NavigateMatch(_)
            | history::Event::Select(_)
            | history::Event::ClearSelection
            | history::Event::ToggleDisclosure(_)
            | history::Event::LoadItem(_)
            | history::Event::LoadOperationDetails { .. }
            | history::Event::Open { .. }
            | history::Event::Completed { .. } => None,
        };
        let command = if let Some(event) = configuration_event {
            command.and(update_configuration(event, model))
        } else {
            command
        };
        let projection_event = match &event {
            history::Event::Refresh => Some(projections::Event::Changed),
            history::Event::Reconnect => Some(projections::Event::Reconnect),
            history::Event::Suspend => Some(projections::Event::Suspend),
            history::Event::Disconnect => Some(projections::Event::Disconnect),
            history::Event::Connect(_)
            | history::Event::SetFilter(_)
            | history::Event::LoadMore
            | history::Event::Search(_)
            | history::Event::SearchMore
            | history::Event::NavigateMatch(_)
            | history::Event::Select(_)
            | history::Event::ClearSelection
            | history::Event::ToggleDisclosure(_)
            | history::Event::LoadItem(_)
            | history::Event::LoadOperationDetails { .. }
            | history::Event::Open { .. }
            | history::Event::Completed { .. } => None,
        };
        let resource_event = match &event {
            history::Event::Refresh => Some(resources::Event::Refresh),
            history::Event::Reconnect => Some(resources::Event::Reconnect),
            history::Event::Suspend => Some(resources::Event::Suspend),
            history::Event::Disconnect => Some(resources::Event::Disconnect),
            history::Event::Connect(_)
            | history::Event::SetFilter(_)
            | history::Event::LoadMore
            | history::Event::Search(_)
            | history::Event::SearchMore
            | history::Event::NavigateMatch(_)
            | history::Event::Select(_)
            | history::Event::ClearSelection
            | history::Event::ToggleDisclosure(_)
            | history::Event::LoadItem(_)
            | history::Event::LoadOperationDetails { .. }
            | history::Event::Open { .. }
            | history::Event::Completed { .. } => None,
        };
        let command = if let Some(event) = resource_event {
            command.and(update_resources(event, model))
        } else {
            command
        };
        let command = if let Some(event) = projection_event {
            command.and(update_projection(event, model))
        } else {
            command
        };
        let history = history::History
            .update(event, &mut model.history)
            .map_event(Event::History);
        command.and(history)
    } else {
        command
    }
}

// Context removal and replacement share the same retirement path. Matching
// domains may already be connected before their shared subscription starts.
fn retire_domains(
    model: &mut Model,
    active: Option<&subscriptions::Context>,
) -> Command<Effect, Event> {
    let mut command = Command::done();
    if active.is_none()
        || model
            .repository
            .context()
            .is_some_and(|context| active != Some(&context.connection))
    {
        command = command.and(
            repository::Repositories
                .update(repository::Event::Disconnect, &mut model.repository)
                .map_event(Event::Repository),
        );
    }
    if active.is_none()
        || model
            .projections
            .context()
            .is_some_and(|context| active != Some(context))
    {
        command = command.and(
            projections::Projections
                .update(projections::Event::Disconnect, &mut model.projections)
                .map_event(Event::Projections),
        );
    }
    if active.is_none()
        || model.sessions.context().is_some_and(|context| {
            !active.is_some_and(|active| {
                context.workspace_id == active.workspace
                    && context.chain == active.chain
                    && context.contributor_id == active.contributor
                    && context.provider == active.provider
            })
        })
    {
        command = command.and(
            sessions::Sessions
                .update(sessions::Event::Disconnect, &mut model.sessions)
                .map_event(Event::Sessions),
        );
    }
    if active.is_none()
        || model.resources.context().is_some_and(|context| {
            !active.is_some_and(|active| resource_subscription_matches(context, active))
        })
    {
        command = command.and(
            resources::Resources
                .update(resources::Event::Disconnect, &mut model.resources)
                .map_event(Event::Resources),
        );
    }
    if active.is_none()
        || model.configuration.context().is_some_and(|context| {
            !active.is_some_and(|active| configuration_subscription_matches(context, active))
        })
    {
        command = command.and(
            configuration::Configuration
                .update(configuration::Event::Disconnect, &mut model.configuration)
                .map_event(Event::Configuration),
        );
    }
    command
}

fn update_projection(event: projections::Event, model: &mut Model) -> Command<Effect, Event> {
    if let projections::Event::Connect(context) = &event
        && ((model.workspace.owns_history()
            && (model.workspace.chain() != Some(context.chain.as_str())
                || model.workspace.workspace_id() != Some(context.workspace.as_str())))
            || model
                .subscriptions
                .context()
                .is_some_and(|active| active != context))
    {
        return Command::done();
    }
    if model.subscriptions.context().is_some() && !model.subscriptions.has_connection() {
        if let projections::Event::Connect(context) = &event {
            model.projections.wait_for_connection(context.clone());
            return render::render();
        }
        if matches!(event, projections::Event::Reconnect) {
            return projections::Projections
                .update(projections::Event::Suspend, &mut model.projections)
                .map_event(Event::Projections);
        }
    }
    let command = projections::Projections
        .update(event, &mut model.projections)
        .map_event(Event::Projections);
    let Some(event) = model.projections.take_history_event() else {
        return command;
    };
    let Some(context) = model.projections.context() else {
        return command;
    };
    let command = command.and(
        workspace::Workspace
            .update(
                workspace::Event::Navigate(workspace::NavigationSection::Activity),
                &mut model.workspace,
            )
            .map_event(Event::Workspace),
    );
    let command = command.and(
        history::History
            .update(
                history::Event::Connect(context.chain.clone()),
                &mut model.history,
            )
            .map_event(Event::History),
    );
    let command = command.and(
        history::History
            .update(
                history::Event::SetFilter(history::Filter::default()),
                &mut model.history,
            )
            .map_event(Event::History),
    );
    command.and(
        history::History
            .update(event, &mut model.history)
            .map_event(Event::History),
    )
}

fn resource_subscription_matches(
    context: &resources::ResourceContext,
    active: &subscriptions::Context,
) -> bool {
    context.workspace_id == active.workspace
        && context.chain == active.chain
        && context.contributor_id == active.contributor
        && context.provider == active.provider
}

fn update_resources(event: resources::Event, model: &mut Model) -> Command<Effect, Event> {
    if let resources::Event::Connect(context) = &event
        && ((model.workspace.owns_history()
            && (model.workspace.chain() != Some(context.chain.as_str())
                || model.workspace.workspace_id() != Some(context.workspace_id.as_str())
                || model.workspace.coordination_mode() != Some(context.mode)))
            || model
                .subscriptions
                .context()
                .is_some_and(|active| !resource_subscription_matches(context, active)))
    {
        return Command::done();
    }
    if model.subscriptions.context().is_some() && !model.subscriptions.has_connection() {
        if let resources::Event::Connect(context) = &event {
            model.resources.wait_for_connection(context.clone());
            return render::render();
        }
        if matches!(event, resources::Event::Reconnect) {
            return resources::Resources
                .update(resources::Event::Suspend, &mut model.resources)
                .map_event(Event::Resources);
        }
    }
    resources::Resources
        .update(event, &mut model.resources)
        .map_event(Event::Resources)
}

fn configuration_subscription_matches(
    context: &configuration::ConfigurationContext,
    active: &subscriptions::Context,
) -> bool {
    context.workspace_id == active.workspace
        && context.chain == active.chain
        && context.contributor_id == active.contributor
        && context.provider == active.provider
}

fn update_configuration(event: configuration::Event, model: &mut Model) -> Command<Effect, Event> {
    if let configuration::Event::Connect(context) = &event
        && ((model.workspace.owns_history()
            && (model.workspace.chain() != Some(context.chain.as_str())
                || model.workspace.workspace_id() != Some(context.workspace_id.as_str())
                || model.workspace.coordination_mode() != Some(context.mode)))
            || model
                .subscriptions
                .context()
                .is_some_and(|active| !configuration_subscription_matches(context, active)))
    {
        return Command::done();
    }
    if model.subscriptions.context().is_some() && !model.subscriptions.has_connection() {
        if let configuration::Event::Connect(context) = &event {
            model.configuration.wait_for_connection(context.clone());
            return render::render();
        }
        if matches!(event, configuration::Event::Reconnect) {
            return configuration::Configuration
                .update(configuration::Event::Suspend, &mut model.configuration)
                .map_event(Event::Configuration);
        }
    }
    configuration::Configuration
        .update(event, &mut model.configuration)
        .map_event(Event::Configuration)
}

fn update_repository(event: repository::Event, model: &mut Model) -> Command<Effect, Event> {
    if let repository::Event::Connect(context) = &event {
        let binding = &context.connection;
        if (model.workspace.owns_history()
            && (model.workspace.chain() != Some(binding.chain.as_str())
                || model.workspace.workspace_id() != Some(binding.workspace.as_str())))
            || model
                .subscriptions
                .context()
                .is_some_and(|active| active != binding)
            || workspace::Workspace
                .view(&model.workspace)
                .repository_binding
                .as_ref()
                .is_some_and(|selected| selected.repository_id != context.repository_id)
        {
            return Command::done();
        }
    }
    if model.subscriptions.context().is_some() && !model.subscriptions.has_connection() {
        if let repository::Event::Connect(context) = &event {
            model.repository.wait_for_connection(context.clone());
            return render::render();
        }
        if matches!(event, repository::Event::Reconnect) {
            return repository::Repositories
                .update(repository::Event::Suspend, &mut model.repository)
                .map_event(Event::Repository);
        }
    }
    let apply_history = !matches!(event, repository::Event::Completed { .. })
        || !model.workspace.owns_history()
        || model.workspace.section() == workspace::NavigationSection::Sessions;
    let mut command = repository::Repositories
        .update(event, &mut model.repository)
        .map_event(Event::Repository);
    let events = model.repository.take_history_events();
    if !apply_history {
        return command;
    }
    if !events.is_empty()
        && let Some(context) = model.repository.context()
    {
        command = command.and(
            history::History
                .update(
                    history::Event::Connect(context.connection.chain.clone()),
                    &mut model.history,
                )
                .map_event(Event::History),
        );
    }
    for event in events {
        command = command.and(
            history::History
                .update(event, &mut model.history)
                .map_event(Event::History),
        );
    }
    command
}

fn navigate_repository(
    section: Option<workspace::NavigationSection>,
    model: &mut Model,
) -> Command<Effect, Event> {
    match section {
        Some(workspace::NavigationSection::Sessions) => {
            let selected = model.repository.selected_session().map(str::to_owned);
            if selected.is_some() {
                return update_repository(repository::Event::SelectSession(selected), model);
            }
        }
        Some(workspace::NavigationSection::Activity) => {
            return history::History
                .update(
                    history::Event::SetFilter(history::Filter::default()),
                    &mut model.history,
                )
                .map_event(Event::History);
        }
        None
        | Some(
            workspace::NavigationSection::Workspace
            | workspace::NavigationSection::Members
            | workspace::NavigationSection::Projections
            | workspace::NavigationSection::ComputeHosts
            | workspace::NavigationSection::ModelProviders
            | workspace::NavigationSection::Settings
            | workspace::NavigationSection::AgentRules,
        ) => {}
    }
    Command::done()
}
