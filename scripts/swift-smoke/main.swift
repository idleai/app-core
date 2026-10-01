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
check(core.protocolVersion() == 4)
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
check(try view(core) == ViewModel(initialized: true, bootstrap: .ready(info), history: idle.history, workspace: idle.workspace))
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
