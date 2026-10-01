//! Client-local interaction state and conservative action presentation.

use std::collections::{BTreeMap, BTreeSet};

use idle_history::requests::RequestTracker;

use super::{
    ComputeFeature, ComputeHostView, ControllerAssignment, ControllerPhase, ControllerView,
    ModelPackage, ModelPackageView, ModelProviderKind, ModelProviderView, ModelTarget,
    ResourceActionStage, ResourceAvailability, ResourceCapability, ResourceContext, ResourceError,
    ResourceLoadState, ResourceMutation, ResourceMutationView, ResourceOperation,
    ResourcePermission, ResourceRecoveryAction, ResourceRetryAdvice, ResourceScope,
    ResourceSnapshot, ServedModelInfo, ServedModelView, ViewModel,
};

/// One client's resource context, navigation and correlated runtime actions.
#[derive(Debug, Default)]
pub struct Model {
    pub(super) context: Option<ResourceContext>,
    pub(super) snapshot: Option<ResourceSnapshot>,
    pub(super) known_grants: BTreeMap<String, super::ResourceGrant>,
    pub(super) load: ResourceLoadState,
    pub(super) selected_host: Option<String>,
    pub(super) selected_provider: Option<String>,
    pub(super) mutations: Vec<ResourceMutationView>,
    // Keep notifications until a status request starts after their arrival.
    pub(super) status_dirty: BTreeSet<String>,
    pub(super) requests: RequestTracker<ResourceOperation>,
    pub(super) now_ms: u64,
    pub(super) refresh_again: bool,
    pub(super) action_error: Option<ResourceError>,
}

impl Model {
    /// Current authenticated workspace context, if selected.
    #[must_use]
    pub fn context(&self) -> Option<&ResourceContext> {
        self.context.as_ref()
    }

    pub(super) fn reset(&mut self) {
        self.requests.clear();
        self.context = None;
        self.snapshot = None;
        self.known_grants.clear();
        self.load = ResourceLoadState::Idle;
        self.selected_host = None;
        self.selected_provider = None;
        self.mutations.clear();
        self.status_dirty.clear();
        self.now_ms = 0;
        self.refresh_again = false;
        self.action_error = None;
    }

    pub(crate) fn wait_for_connection(&mut self, context: ResourceContext) {
        if let Err(error) = super::validation::context(&context) {
            self.action_error = Some(error);
            return;
        }
        if self.context() != Some(&context) {
            self.reset();
            self.context = Some(context);
        }
        self.suspend();
    }

    pub(super) fn suspend(&mut self) {
        self.requests.clear();
        self.status_dirty.clear();
        self.refresh_again = false;
        self.load = ResourceLoadState::Suspended;
        for mutation in &mut self.mutations {
            mutation.in_flight = false;
            if mutation.pending {
                mutation.error = Some(super::validation::uncertain(
                    "Connection interrupted; check the original action status",
                ));
            }
        }
    }

    pub(super) fn ready(&self) -> bool {
        self.load == ResourceLoadState::Ready
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.member_active)
    }

    fn permitted(&self, scope: &ResourceScope, permission: ResourcePermission) -> bool {
        self.ready()
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.grants.iter().any(|grant| {
                    grant.active
                        && &grant.scope == scope
                        && grant.permissions.contains(&permission)
                        && grant
                            .expires_at_ms
                            .is_none_or(|expiry| self.now_ms < expiry)
                })
            })
    }

    fn health(&self, health: &super::ResourceHealth) -> ResourceAvailability {
        if self.load == ResourceLoadState::Ready {
            health.at(self.now_ms)
        } else {
            ResourceAvailability::Unknown
        }
    }

    pub(super) fn host_availability(&self, id: &str) -> ResourceAvailability {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.hosts.iter().find(|host| host.id == id))
            .map_or(ResourceAvailability::Unknown, |host| {
                self.health(&host.health)
            })
    }

    fn provider_availability(&self, id: &str) -> ResourceAvailability {
        let Some(provider) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.providers.iter().find(|provider| provider.id == id))
        else {
            return ResourceAvailability::Unknown;
        };
        let health = self.health(&provider.health);
        match &provider.kind {
            ModelProviderKind::External => health,
            ModelProviderKind::Local { host_id, .. } => {
                combine(health, self.host_availability(host_id))
            }
        }
    }

    fn model_availability(&self, model: &ServedModelInfo) -> ResourceAvailability {
        combine(
            self.health(&model.health),
            self.provider_availability(&model.key.provider_id),
        )
    }

    pub(super) fn conflicts(&self, mutation: &ResourceMutation) -> bool {
        self.mutations.iter().any(|pending| {
            pending.pending
                && match (&pending.mutation, mutation) {
                    (
                        ResourceMutation::ConnectHost { host_id: first },
                        ResourceMutation::ConnectHost { host_id: second },
                    ) => first == second,
                    (
                        ResourceMutation::SelectModel { target: first, .. },
                        ResourceMutation::SelectModel { target: second, .. },
                    ) => first.session_id == second.session_id,
                    (
                        ResourceMutation::InstallModel(first),
                        ResourceMutation::InstallModel(second),
                    ) => first.host_id == second.host_id,
                    (
                        ResourceMutation::ConnectHost { .. }
                        | ResourceMutation::SelectModel { .. }
                        | ResourceMutation::InstallModel(_),
                        _,
                    ) => false,
                }
        })
    }

    pub(super) fn host_actions(&self, id: &str) -> Vec<ResourcePermission> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        if self.host_availability(id) != ResourceAvailability::Available {
            return Vec::new();
        }
        let scope = ResourceScope::Host(id.into());
        let mut actions = Vec::new();
        if snapshot.runtime.capabilities.connect_host == ResourceCapability::Available
            && self.permitted(&scope, ResourcePermission::ConnectHost)
            && !self.conflicts(&ResourceMutation::ConnectHost { host_id: id.into() })
        {
            actions.push(ResourcePermission::ConnectHost);
        }
        if snapshot.runtime.capabilities.install_model == ResourceCapability::Available
            && self.permitted(&scope, ResourcePermission::InstallModel)
            && snapshot
                .hosts
                .iter()
                .any(|host| host.id == id && host.features.contains(&ComputeFeature::LocalModels))
            && snapshot.runtime.packages.iter().any(|package| {
                package.host_id == id
                    && !self.conflicts(&ResourceMutation::InstallModel(package.clone()))
            })
        {
            actions.push(ResourcePermission::InstallModel);
        }
        actions
    }

    fn provider_actions(&self, id: &str) -> Vec<ResourcePermission> {
        if self.provider_availability(id) == ResourceAvailability::Available
            && self.permitted(
                &ResourceScope::Provider(id.into()),
                ResourcePermission::UseModels,
            )
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.runtime.capabilities.select_model == ResourceCapability::Available
            })
        {
            vec![ResourcePermission::UseModels]
        } else {
            Vec::new()
        }
    }

    fn target_available(&self, target: &ModelTarget) -> bool {
        self.host_availability(&target.host_id) == ResourceAvailability::Available
            && target.control_epoch.is_none_or(|_| {
                self.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.controller.lease.as_ref().is_some_and(|lease| {
                        lease.target == *target
                            && lease.acquired_at_ms <= self.now_ms
                            && self.now_ms < lease.expires_at_ms
                    })
                })
            })
    }

    fn selectable_for(&self, model: &ServedModelInfo) -> Vec<ModelTarget> {
        if self.model_availability(model) != ResourceAvailability::Available
            || self.provider_actions(&model.key.provider_id).is_empty()
        {
            return Vec::new();
        }
        self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            snapshot
                .runtime
                .selections
                .iter()
                .filter(|selection| {
                    selection.capability == ResourceCapability::Available
                        && selection.selected.as_ref() != Some(&model.key)
                        && self.target_available(&selection.target)
                        && !self.conflicts(&ResourceMutation::SelectModel {
                            target: selection.target.clone(),
                            model: model.key.clone(),
                        })
                })
                .map(|selection| selection.target.clone())
                .collect()
        })
    }

    fn can_install(&self, package: &ModelPackage) -> bool {
        self.host_actions(&package.host_id)
            .contains(&ResourcePermission::InstallModel)
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.runtime.packages.contains(package))
    }

    pub(super) fn allowed(&self, mutation: &ResourceMutation) -> bool {
        match mutation {
            ResourceMutation::ConnectHost { host_id } => self
                .host_actions(host_id)
                .contains(&ResourcePermission::ConnectHost),
            ResourceMutation::InstallModel(package) => self.can_install(package),
            ResourceMutation::SelectModel { target, model } => {
                self.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.models.iter().any(|entry| {
                        entry.key == *model && self.selectable_for(entry).contains(target)
                    })
                })
            }
        }
    }

    fn retry_permitted(&self, mutation: &ResourceMutation) -> bool {
        let Some(snapshot) = &self.snapshot else {
            return false;
        };
        let capabilities = &snapshot.runtime.capabilities;
        match mutation {
            ResourceMutation::ConnectHost { host_id } => {
                capabilities.connect_host == ResourceCapability::Available
                    && self.host_availability(host_id) == ResourceAvailability::Available
                    && self.permitted(
                        &ResourceScope::Host(host_id.clone()),
                        ResourcePermission::ConnectHost,
                    )
            }
            ResourceMutation::InstallModel(package) => {
                capabilities.install_model == ResourceCapability::Available
                    && self.host_availability(&package.host_id) == ResourceAvailability::Available
                    && snapshot.runtime.packages.contains(package)
                    && snapshot.hosts.iter().any(|host| {
                        host.id == package.host_id
                            && host.features.contains(&ComputeFeature::LocalModels)
                    })
                    && self.permitted(
                        &ResourceScope::Host(package.host_id.clone()),
                        ResourcePermission::InstallModel,
                    )
            }
            ResourceMutation::SelectModel { target, model } => {
                capabilities.select_model == ResourceCapability::Available
                    && self.target_available(target)
                    && self.permitted(
                        &ResourceScope::Provider(model.provider_id.clone()),
                        ResourcePermission::UseModels,
                    )
                    && snapshot.runtime.selections.iter().any(|selection| {
                        selection.target == *target
                            && selection.capability == ResourceCapability::Available
                    })
                    && snapshot.models.iter().any(|entry| {
                        entry.key == *model
                            && self.model_availability(entry) == ResourceAvailability::Available
                    })
            }
        }
    }

    pub(super) fn recovery(&self, mutation: &ResourceMutationView) -> Vec<ResourceRecoveryAction> {
        if !self.ready() || mutation.in_flight || !mutation.pending {
            return Vec::new();
        }
        let retry = mutation.error.as_ref().is_some_and(|error| matches!(error.retry,
            ResourceRetryAdvice::SameRequest { not_before_ms } if not_before_ms.is_none_or(|time| self.now_ms >= time)));
        let mut actions = vec![ResourceRecoveryAction::CheckStatus];
        if retry
            && self.now_ms < mutation.request.expires_at_ms
            && self.retry_permitted(&mutation.mutation)
        {
            actions.push(ResourceRecoveryAction::Retry);
        }
        actions
    }

    fn controller_view(&self, snapshot: &ResourceSnapshot) -> ControllerView {
        let mut view = ControllerView {
            ownership: snapshot.controller.clone(),
            ..ControllerView::default()
        };
        if self.load != ResourceLoadState::Ready {
            return view;
        }
        let Some(lease) = &snapshot.controller.lease else {
            view.assignment = ControllerAssignment::Unassigned;
            return view;
        };
        if self.now_ms >= lease.expires_at_ms {
            view.assignment = ControllerAssignment::Expired;
        } else if self.now_ms >= lease.acquired_at_ms {
            view.assignment = ControllerAssignment::Assigned;
            if let Some(runtime) = &snapshot.runtime.controller {
                view.availability = combine(
                    self.health(&runtime.health),
                    self.host_availability(&lease.target.host_id),
                );
                if view.availability == ResourceAvailability::Available
                    || (matches!(runtime.phase, ControllerPhase::Failed(_))
                        && self.health(&runtime.health) != ResourceAvailability::Unknown)
                {
                    view.phase = runtime.phase.clone();
                }
            }
        }
        view
    }

    pub(super) fn view(&self) -> ViewModel {
        let mut view = ViewModel {
            context: self.context.clone(),
            load: self.load.clone(),
            selected_host: self.selected_host.clone(),
            selected_provider: self.selected_provider.clone(),
            action_error: self.action_error.clone(),
            mutations: self
                .mutations
                .iter()
                .map(|mutation| {
                    let mut result = mutation.clone();
                    result.recovery = self.recovery(mutation);
                    result
                })
                .collect(),
            ..ViewModel::default()
        };
        let Some(snapshot) = &self.snapshot else {
            return view;
        };
        if self.ready() {
            view.capabilities = snapshot.runtime.capabilities.clone();
        }
        view.hosts = snapshot
            .hosts
            .iter()
            .map(|host| ComputeHostView {
                host: host.clone(),
                availability: self.host_availability(&host.id),
                actions: self.host_actions(&host.id),
            })
            .collect();
        view.providers = snapshot
            .providers
            .iter()
            .map(|provider| ModelProviderView {
                provider: provider.clone(),
                availability: self.provider_availability(&provider.id),
                actions: self.provider_actions(&provider.id),
            })
            .collect();
        view.models = snapshot
            .models
            .iter()
            .map(|model| ServedModelView {
                model: model.clone(),
                availability: self.model_availability(model),
                selectable_for: self.selectable_for(model),
            })
            .collect();
        view.packages = snapshot
            .runtime
            .packages
            .iter()
            .map(|package| ModelPackageView {
                package_info: package.clone(),
                can_install: self.can_install(package),
            })
            .collect();
        view.selections.clone_from(&snapshot.runtime.selections);
        if !self.ready() {
            for selection in &mut view.selections {
                selection.capability = ResourceCapability::Unavailable;
            }
        }
        view.controller = self.controller_view(snapshot);
        view
    }
}

fn combine(first: ResourceAvailability, second: ResourceAvailability) -> ResourceAvailability {
    if first == ResourceAvailability::Unavailable || second == ResourceAvailability::Unavailable {
        ResourceAvailability::Unavailable
    } else if first == ResourceAvailability::Available && second == ResourceAvailability::Available
    {
        ResourceAvailability::Available
    } else {
        ResourceAvailability::Unknown
    }
}

pub(super) fn terminal(stage: &ResourceActionStage) -> bool {
    matches!(
        stage,
        ResourceActionStage::Succeeded | ResourceActionStage::Failed(_)
    )
}
