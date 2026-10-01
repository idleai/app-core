// Exercise BoltFFI's native ABI using Facet-generated payload codecs.
import AppCoreBindings
import AppTypes
import Foundation

enum SmokeError: Error { case acceptedInvalidCall, missingEffect }

func check(_ condition: Bool) {
    precondition(condition, "Swift binding contract failed")
}

func rejected(_ action: () throws -> Void) throws {
    do { try action() } catch is BindingError { return }
    throw SmokeError.acceptedInvalidCall
}

func view(_ core: AppCore) throws -> ViewModel {
    try ViewModel.bincodeDeserialize(input: Array(core.view()))
}

func send(_ core: AppCore, _ event: AppTypes.Event) throws -> [EffectRequest] {
    try EffectBatch.bincodeDeserialize(
        input: Array(core.processEvent(event: Data(event.bincodeSerialize())))
    ).requests
}

func request(_ effects: [EffectRequest], _ kind: EffectFfi) throws -> UInt32 {
    guard let effect = effects.first(where: { $0.effect == kind }) else {
        throw SmokeError.missingEffect
    }
    return effect.id
}

let core = AppCore()
let other = AppCore()
let idle = try view(core)
check(!idle.initialized && idle.bootstrap == .idle && idle.history.chain == nil)
check(core.protocolVersion() == 10)
check(try view(core) == idle)
try rejected { _ = try core.processEvent(event: Data("invalid".utf8)) }
try rejected { _ = try core.processEvent(event: Data([1, 0, 0, 0, 1, 0, 0, 0])) }
let effects = try send(core, .start)
let id = try request(effects, .hostInfo)
let renderID = try request(effects, .render)
check(try view(core).bootstrap == .loading)
check(try send(core, .start).isEmpty)
let info = HostInfo(name: "Swift host 🌍", version: "1.0")
let success = Data(try HostInfoResponse.ok(info).bincodeSerialize())
try rejected { _ = try other.handleResponse(id: id, response: success) }
try rejected { _ = try core.handleResponse(id: renderID, response: success) }
try rejected { _ = try core.handleResponse(id: UInt32.max, response: success) }
try rejected { _ = try core.handleResponse(id: id, response: Data([0])) }
try rejected { _ = try core.handleResponse(id: id, response: success + Data([0])) }
let renders = try EffectBatch.bincodeDeserialize(
    input: Array(core.handleResponse(id: id, response: success))
).requests
check(renders.map(\.effect) == [.render])
check(try view(core) == ViewModel(initialized: true, bootstrap: .ready(info), history: idle.history, workspace: idle.workspace, subscriptions: idle.subscriptions, sessions: idle.sessions, projections: idle.projections, resources: idle.resources, configuration: idle.configuration))
try rejected { _ = try core.handleResponse(id: id, response: success) }
check(try view(other) == idle)

let failedID = try request(send(other, .start), .hostInfo)
let error = EffectError(message: "Host unavailable")
let failure = Data(try HostInfoResponse.err(error).bincodeSerialize())
_ = try other.handleResponse(id: failedID, response: failure)
check(try view(other).bootstrap == .failed(error))
let retryID = try request(send(other, .bootstrap(.load)), .hostInfo)
check(retryID != failedID)
try rejected { _ = try other.handleResponse(id: failedID, response: success) }
_ = try other.handleResponse(id: retryID, response: success)
check(try view(other).bootstrap == .ready(info))
print("Swift + BoltFFI + Facet: event -> effect -> result -> typed view PASS")

let historyEffects = try send(core, .history(.connect("chain")))
guard let historyRequest = historyEffects.first(where: {
    if case .history = $0.effect { return true }; return false
}) else { throw SmokeError.missingEffect }
if case .history(let query) = historyRequest.effect { check(query.chain == "chain") }
check(try view(core).history.paging.state == .loading)
let historyResponse = QueryResponse.ok(.history(HistoryPage(observations: [], nextAfter: nil, scanned: 0)))
_ = try core.handleResponse(id: historyRequest.id, response: Data(historyResponse.bincodeSerialize()))
check(try view(core).history.paging.state == .ready)
check(try view(core).history.paging.exhausted)
check(try view(other).history.chain == nil)
print("Swift history event -> engine-query effect -> typed page PASS")

let missingOperation = String(repeating: "a", count: 64)
let detailEffects = try send(core, .history(.loadOperationDetails(operation: missingOperation, refresh: false)))
let detailID = try request(detailEffects, .history(Query(chain: "chain", action: .operationDetails(operation: missingOperation))))
let details = OperationDetails(operation: missingOperation, status: .missing,
                               observation: nil, records: [], fields: [], comparison: nil)
let detailResponse = QueryResponse.ok(.operationDetails(details))
_ = try core.handleResponse(id: detailID, response: Data(detailResponse.bincodeSerialize()))
check(try view(core).history.operationDetails.first?.state == .ready(details))
print("Swift operation records/content request -> result PASS")

func workspaceSmoke(_ mode: WorkspaceMode) throws {
    let client = AppCore()
    let listID = try request(send(client, .workspace(.load)), .workspace(.list))
    let repo = RepositoryInfo(id: "repo", name: "Repository", remote: nil)
    let info = WorkspaceInfo(id: "workspace", name: "Workspace 🌍", chain: "logical-chain",
                             revision: UInt64.max, mode: mode, repositories: [repo])
    let list = WorkspaceResponse.ok(.directory([info]))
    _ = try client.handleResponse(id: listID, response: Data(list.bincodeSerialize()))
    check(try view(client).workspace.workspaces == [info])
    let selected = try send(client, .workspace(.selectWorkspace("workspace")))
    let snapshotID = try request(selected, .workspace(.snapshot(workspaceId: "workspace", mode: mode)))
    guard let engine = selected.first(where: {
        if case .history = $0.effect { return true }; return false
    }) else { throw SmokeError.missingEffect }
    if case .history(let query) = engine.effect { check(query.chain == "logical-chain") }
    let page = QueryResponse.ok(.history(HistoryPage(observations: [], nextAfter: nil, scanned: 0)))
    _ = try client.handleResponse(id: engine.id, response: Data(page.bincodeSerialize()))
    let member = MemberInfo(contributorId: "alice", displayName: "Alice", revision: UInt64.max,
                            role: .member, status: .active)
    let snapshot = WorkspaceSnapshot(workspace: info, members: [member], hostIds: ["host"], providerIds: ["provider"])
    let response = WorkspaceResponse.ok(.snapshot(snapshot))
    let followup = try EffectBatch.bincodeDeserialize(input: Array(client.handleResponse(
        id: snapshotID, response: Data(response.bincodeSerialize())))).requests
    let presenceID = try request(followup, .workspace(.presence(workspaceId: "workspace", mode: mode)))
    let entry = PresenceEntry(connectionId: "connection", contributorId: "alice", status: .online,
                              repositoryId: "repo", branch: "main", file: "src/lib.rs", hostId: "host",
                              summary: "Editing", observedAtMs: 100, validUntilMs: 200)
    let presence = WorkspaceResponse.ok(.presence(PresenceSnapshot(workspaceId: "workspace", asOfMs: 100, entries: [entry])))
    _ = try client.handleResponse(id: presenceID, response: Data(presence.bincodeSerialize()))
    check(try view(client).workspace.members.first?.presence == .online)
    check(try view(client).workspace.repositoryBinding?.chain == "logical-chain")
    _ = try send(client, .workspace(.navigate(.agentRules)))
    check(try view(client).workspace.section == .agentRules)
    _ = try send(client, .workspace(.tick(200)))
    check(try view(client).workspace.members.first?.presence == .unknown)
    let retryID = try request(send(client, .workspace(.refreshPresence)), .workspace(.presence(workspaceId: "workspace", mode: mode)))
    let error = WorkspaceError(kind: .unavailable, message: "Presence unavailable")
    _ = try client.handleResponse(id: retryID, response: Data(WorkspaceResponse.err(error).bincodeSerialize()))
    check(try view(client).workspace.presenceState == .failed(error))
    _ = try send(client, .workspace(.disconnect))
    check(try view(client).history.chain == nil)
}

try workspaceSmoke(.standalone)
try workspaceSmoke(.managed)
print("Swift workspace selection + members + presence in both modes PASS")

func subscriptionSmoke() throws {
    let client = AppCore()
    let context = Context(provider: "fixture", workspace: "workspace", contributor: "alice", chain: "chain")
    let join = try request(send(client, .subscriptions(.connect(context))),
                           .subscription(SubscriptionOperation(context: context, action: .join)))
    let follow = try EffectBatch.bincodeDeserialize(input: Array(client.handleResponse(
        id: join, response: Data(SubscriptionResponse.ok(.joined(connection: "connection")).bincodeSerialize())))).requests
    check(try view(client).subscriptions.status == .reconciling)
    guard let read = follow.first(where: {
        if case .history = $0.effect { return true }; return false
    }) else { throw SmokeError.missingEffect }
    let watch = try request(follow, .subscription(SubscriptionOperation(context: context, action: .watch(connection: "connection"))))
    let replacement = Reconciled(history: [HistoryPage(observations: [], nextAfter: nil, scanned: 0)], search: [], items: [], details: [])
    _ = try client.handleResponse(id: read.id, response: Data(QueryResponse.ok(.reconciled(replacement)).bincodeSerialize()))
    check(try view(client).subscriptions.status == .live)
    check(try view(client).history.reconciliation == .ready)
    _ = try send(client, .subscriptions(.reconnect))
    let before = try view(client)
    _ = try client.handleResponse(id: watch, response: Data(SubscriptionResponse.ok(.changed).bincodeSerialize()))
    check(try view(client) == before)
    check(before.subscriptions.status == .connecting)
}

try subscriptionSmoke()
print("Swift subscription join + snapshot + stale watch PASS")

func sessionSmoke(_ mode: WorkspaceMode) throws {
    let client = AppCore()
    let context = SessionContext(provider: "fixture", workspaceId: "workspace", contributorId: "bob", chain: "chain", mode: mode)
    let load = try request(send(client, .sessions(.connect(context))), .session(SessionOperation(context: context, action: .snapshot)))
    let actor = SessionContributor(contributorId: "bob", issuer: "peer", subject: "bob-key")
    let cursor = SessionCursor(workspaceId: "workspace", contributorId: "bob", streamId: "stream", position: 100)
    let binding = SessionItemBinding(chain: "chain", item: String(repeating: "c", count: 64))
    let session = SessionInfo(id: "session", owner: "alice", title: "Shared 🌍", kind: .runner, revision: 1,
                              runtime: SessionRuntimeBinding(hostId: "host", runtimeId: "runtime"), parent: nil, history: binding)
    let grant = SessionGrant(id: "grant", sessionId: "session", grantee: "bob", grantedBy: "alice", permissions: [.observe, .submitInput], expiresAtMs: 2000, revision: 1, status: .active)
    let members = [MemberInfo(contributorId: "alice", displayName: "Alice", revision: 1, role: .owner, status: .active),
                   MemberInfo(contributorId: "bob", displayName: "Bob", revision: 1, role: .member, status: .active)]
    let snapshot = SessionSnapshot(context: context, cursor: cursor, contributor: actor,
        capabilities: SessionCapabilities(create: .unavailable, input: .available, share: .available),
        members: members, sessions: [session], grants: [grant], inputs: [], nowMs: 1000)
    let follow = try EffectBatch.bincodeDeserialize(input: Array(client.handleResponse(id: load, response: Data(SessionResponse.ok(.snapshot(snapshot)).bincodeSerialize())))).requests
    let watch = try request(follow, .session(SessionOperation(context: context, action: .watch(after: cursor))))
    check(try view(client).sessions.sessions.first?.relationship == .invited)
    _ = try send(client, .sessions(.select("session")))
    check(try view(client).sessions.selectedHistory == binding)
    let mutationID = SessionMutationId(requestId: "input", expiresAtMs: 2000)
    let attribution = SessionRequest(workspaceId: "workspace", contributor: actor, mutation: mutationID)
    let submit = try request(send(client, .sessions(.submit(id: mutationID, text: "Prompt 🌍\n"))),
        .session(SessionOperation(context: context, action: .mutate(request: attribution, mutation: .submit(sessionId: "session", text: "Prompt 🌍\n")))))
    let key = SessionRequestKey(workspaceId: "workspace", contributorId: "bob", requestId: "input")
    let receipt = SessionReceipt(request: key, receivedAtMs: 1000, retryUntilMs: 2500)
    _ = try client.handleResponse(id: submit, response: Data(SessionResponse.ok(.acknowledged(.received(receipt))).bincodeSerialize()))
    check(try view(client).sessions.prompts.first?.runtime == nil)
    check(try view(client).sessions.prompts.first?.contributor == actor)
    let delivery = SessionDelivery(acceptedAtMs: 1100, order: UInt64.max, orderedAtMs: 1150)
    let update = SessionInputUpdate(input: SessionInputRef(sessionId: "session", request: key), contributor: actor, runtimeId: "runtime", revision: 4,
                                    state: .completed(delivery: delivery, completedAtMs: 1500, outcome: .succeeded))
    let through = SessionCursor(workspaceId: "workspace", contributorId: "bob", streamId: "stream", position: 101)
    let changes = SessionChanges(after: cursor, through: through, events: [SessionChangeEvent(position: 101, change: .input(update))], nowMs: 1500)
    _ = try client.handleResponse(id: watch, response: Data(SessionResponse.ok(.changes(changes)).bincodeSerialize()))
    check(try view(client).sessions.prompts.first?.runtime == update)
    check(try view(client).sessions.prompts.first?.text == "Prompt 🌍\n")
    _ = try send(client, .sessions(.tick(2000)))
    check(try view(client).sessions.selected == nil)
    check(try view(client).sessions.prompts.isEmpty)
}

try sessionSmoke(.standalone)
try sessionSmoke(.managed)
print("Swift session attribution + receipt + runtime completion + expiry in both modes PASS")

func projectionSmoke() throws {
    let client = AppCore()
    let context = Context(provider: "managed", workspace: "workspace", contributor: "alice", chain: "chain")
    let load = try request(send(client, .projections(.connect(context))), .projection(ProjectionQuery(context: context, limit: 100)))
    let source = ProjectionReference(observation: String(repeating: "ab", count: 32), item: String(repeating: "cd", count: 32), recordHash: String(repeating: "ef", count: 32))
    let row = ProjectionRow(key: "stable-task", title: "Check 🌍", summary: "Exact details\n", status: "provider/active", labels: ["supplied"], sources: [source], related: [])
    let freshness = ProjectionFreshness(status: .current, generatedAtMs: 1000, checkpoint: "opaque/checkpoint")
    let inputs = [ProjectionKind.activity, .task, .error, .triage, .needInput].map {
        ProjectionInput(kind: $0, freshness: freshness, availability: .complete, total: 1, rows: [row], gaps: [])
    }
    let snapshot = ProjectionSnapshot(version: 1, workspaceId: "workspace", chain: "chain", inputs: inputs)
    _ = try client.handleResponse(id: load, response: Data(ProjectionResponse.ok(snapshot).bincodeSerialize()))
    check(try view(client).projections.tasks.rows.first?.sources.first == source)
    check(try view(client).projections.needInput.freshness == freshness)
    _ = try send(client, .projections(.setFilter(kind: .task, filter: ProjectionFilter(text: "absent", status: nil, labels: []))))
    check(try view(client).projections.tasks.visibleCount == 0)
    check(try view(client).projections.tasks.total == 1)
    _ = try send(client, .projections(.refresh))
    check(try view(client).projections.tasks.freshness.status == .stale)
}

try projectionSmoke()
print("Swift projection inputs + references + freshness + filtering PASS")

func resourceSmoke(_ mode: WorkspaceMode) throws {
    let client = AppCore()
    let context = ResourceContext(provider: "resources", workspaceId: "workspace", contributorId: "bob", chain: "chain", mode: mode)
    let load = try request(send(client, .resources(.connect(context))), .resource(ResourceOperation(context: context, kind: .snapshot)))
    let health = ResourceHealth(availability: .available, observedAtMs: 900, validUntilMs: 2500)
    let host = ComputeHostInfo(id: "host", owner: "alice", name: "Shared host", revision: 1, features: [.sessions, .localModels], health: health)
    let provider = ModelProviderInfo(id: "provider", owner: "alice", name: "Local provider", revision: 1, kind: .local(hostId: "host", runtimeId: "runtime"), health: health)
    let model = ServedModelInfo(key: ModelKey(providerId: "provider", modelId: "model"), name: "Coding model", revision: 1, features: [.text, .tools], health: health)
    let target = ModelTarget(sessionId: "control", hostId: "host", runtimeId: "runtime", controlEpoch: UInt64.max)
    let ownership = ControllerOwnership(lastEpoch: UInt64.max, lease: ControllerLease(target: target, acquiredAtMs: 800, expiresAtMs: 3000))
    let package = ModelPackage(packageId: "local-coder", name: "Coder 🌍", hostId: "host", runtimeId: "runtime")
    let runtime = ResourceRuntimeInfo(capabilities: ResourceCapabilities(connectHost: .available, selectModel: .available, installModel: .available), selections: [ModelSelection(target: target, selected: nil, capability: .available)], packages: [package], controller: ControllerRuntime(target: target, phase: .running, health: health))
    let grants = [ResourceGrant(id: "compute", revision: 1, scope: .host("host"), permissions: [.connectHost, .installModel], expiresAtMs: nil, active: true), ResourceGrant(id: "provider", revision: 1, scope: .provider("provider"), permissions: [.useModels], expiresAtMs: nil, active: true)]
    let snapshot = ResourceSnapshot(context: context, streamId: "stream", position: UInt64.max, nowMs: 1000, memberActive: true, hosts: [host], providers: [provider], models: [model], grants: grants, controller: ownership, runtime: runtime)
    _ = try client.handleResponse(id: load, response: Data(ResourceResponse.ok(.snapshot(snapshot)).bincodeSerialize()))
    check(try view(client).resources.controller.ownership.lastEpoch == UInt64.max)
    check(try view(client).resources.controller.phase == .running)
    check(try view(client).resources.models.first?.selectableFor == [target])
    check(try view(client).resources.packages.first?.canInstall == true)
    check(try view(client).resources.packages.first?.packageInfo.name == "Coder 🌍")
    let identity = ResourceRequest(requestId: "install", expiresAtMs: 2000)
    let mutation = ResourceMutation.installModel(package)
    let install = try request(send(client, .resources(.execute(request: identity, mutation: mutation))), .resource(ResourceOperation(context: context, kind: .mutate(request: identity, mutation: mutation))))
    check(try view(client).resources.mutations.first?.pending == true)
    check(try view(client).resources.packages.first?.canInstall == false)
    _ = try client.handleResponse(id: install, response: Data(ResourceResponse.ok(.progress(ResourceProgress(context: context, request: identity, revision: 1, stage: .received))).bincodeSerialize()))
    check(try view(client).resources.mutations.first?.pending == true)
    let status = try request(send(client, .resources(.checkStatus("install"))), .resource(ResourceOperation(context: context, kind: .status(identity))))
    let failure = ResourceError(code: .unavailable, message: "Insufficient disk space 🌍", retry: .never)
    _ = try client.handleResponse(id: status, response: Data(ResourceResponse.ok(.progress(ResourceProgress(context: context, request: identity, revision: 2, stage: .failed(failure)))).bincodeSerialize()))
    check(try view(client).resources.mutations.first?.pending == false)
    check(try view(client).resources.mutations.first?.progress?.stage == .failed(failure))
    _ = try send(client, .resources(.advanceClock(3000)))
    check(try view(client).resources.controller.assignment == .expired)
    check(try view(client).resources.models.first?.availability == .unknown)

    let restored = AppCore()
    let restoredLoad = try request(send(restored, .resources(.connect(context))), .resource(ResourceOperation(context: context, kind: .snapshot)))
    _ = try restored.handleResponse(id: restoredLoad, response: Data(ResourceResponse.ok(.snapshot(snapshot)).bincodeSerialize()))
    _ = try send(restored, .resources(.advanceClock(2100)))
    let recover = try request(send(restored, .resources(.restore(context: context, request: identity, mutation: mutation))), .resource(ResourceOperation(context: context, kind: .status(identity))))
    check(try view(restored).resources.mutations.first?.request == identity)
    check(try view(restored).resources.mutations.first?.pending == true)
    check(try view(restored).resources.mutations.first?.progress == nil)
    check(try send(restored, .resources(.restore(context: context, request: identity, mutation: mutation))).isEmpty)
    _ = try restored.handleResponse(id: recover, response: Data(ResourceResponse.ok(.progress(ResourceProgress(context: context, request: identity, revision: 2, stage: .failed(failure)))).bincodeSerialize()))
    check(try view(restored).resources.mutations.first?.pending == false)
    check(try view(restored).resources.mutations.first?.progress?.stage == .failed(failure))
}

try resourceSmoke(.standalone)
try resourceSmoke(.managed)
print("Swift resource actions + progress + restoration + controller epochs/expiry in both modes PASS")

func configurationSmoke(_ mode: WorkspaceMode) throws {
    let client = AppCore()
    let context = ConfigurationContext(provider: "configuration", workspaceId: "workspace", contributorId: "alice", chain: "chain", mode: mode)
    let loads = try send(client, .configuration(.connect(context)))
    for document in [ConfigurationDocument.settings, .agentRules] {
        let operation = ConfigurationOperation(context: context, document: document, action: .load)
        let load = try request(loads, .configuration(operation))
        let record = ConfigurationRecord(revision: UInt64.max - 1, value: ConfigurationValue(schemaVersion: 1, json: "{}"))
        let snapshot = ConfigurationSnapshot(context: context, document: document, record: record, canEdit: true)
        _ = try client.handleResponse(id: load, response: Data(ConfigurationResponse.ok(.loaded(snapshot)).bincodeSerialize()))
    }
    check(try view(client).configuration.settings.baseRevision == UInt64.max - 1)
    let json = "{\"name\":\"Workspace 🌍\"}"
    _ = try send(client, .configuration(.edit(document: .settings, json: json)))
    let identity = ConfigurationRequest(requestId: "settings-save", expiresAtMs: 1000)
    let value = ConfigurationValue(schemaVersion: 1, json: json)
    let save = ConfigurationSave(request: identity, expectedRevision: UInt64.max - 1, value: value)
    let operation = ConfigurationOperation(context: context, document: .settings, action: .save(save))
    let pending = try request(send(client, .configuration(.save(document: .settings, request: identity))), .configuration(operation))
    check(try view(client).configuration.settings.save == .saving)
    _ = try send(client, .configuration(.edit(document: .settings, json: "{\"newer\":true}")))
    let snapshot = ConfigurationSnapshot(context: context, document: .settings, record: ConfigurationRecord(revision: UInt64.max, value: value), canEdit: true)
    _ = try client.handleResponse(id: pending, response: Data(ConfigurationResponse.ok(.saved(request: identity, snapshot: snapshot)).bincodeSerialize()))
    let editor = try view(client).configuration.settings
    check(editor.save == .saved(UInt64.max) && editor.dirty && editor.pending == nil)
    check(editor.draft.json == "{\"newer\":true}" && editor.baseRevision == UInt64.max)
    _ = try send(client, .configuration(.edit(document: .agentRules, json: "[]")))
    check(try view(client).configuration.agentRules.validationError?.kind == .invalidInput)
    check(try view(client).configuration.agentRules.actions.contains(.save) == false)
    _ = try send(client, .configuration(.discard(.agentRules)))
    check(try view(client).configuration.agentRules.dirty == false)
}
try configurationSmoke(.standalone)
try configurationSmoke(.managed)
print("Swift configuration drafts + conditional saves + full revisions in both modes PASS")
