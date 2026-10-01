// Exercise the generated Kotlin API, JNI glue and Facet payload codecs together.
import ai.idle.appcore.bindings.AppCore
import ai.idle.appcore.bindings.BindingError
import ai.idle.appcore.types.BootstrapEvent
import ai.idle.appcore.types.EffectBatch
import ai.idle.appcore.types.EffectError
import ai.idle.appcore.types.EffectFfi
import ai.idle.appcore.types.EffectRequest
import ai.idle.appcore.types.Event
import ai.idle.appcore.types.HostInfo
import ai.idle.appcore.types.HostInfoResponse
import ai.idle.appcore.types.HistoryEvent
import ai.idle.appcore.types.HistoryPage
import ai.idle.appcore.types.OperationDetails
import ai.idle.appcore.types.OperationDetailsState
import ai.idle.appcore.types.Query
import ai.idle.appcore.types.QueryAction
import ai.idle.appcore.types.QueryResponse
import ai.idle.appcore.types.QueryResult
import ai.idle.appcore.types.RequestState
import ai.idle.appcore.types.RecordLookupStatus
import ai.idle.appcore.types.LoadState
import ai.idle.appcore.types.ViewModel
import ai.idle.appcore.types.MemberInfo
import ai.idle.appcore.types.MemberRole
import ai.idle.appcore.types.MemberStatus
import ai.idle.appcore.types.NavigationSection
import ai.idle.appcore.types.PresenceEntry
import ai.idle.appcore.types.PresenceSnapshot
import ai.idle.appcore.types.PresenceStatus
import ai.idle.appcore.types.RepositoryInfo
import ai.idle.appcore.types.WorkspaceError
import ai.idle.appcore.types.WorkspaceErrorKind
import ai.idle.appcore.types.WorkspaceEvent
import ai.idle.appcore.types.WorkspaceInfo
import ai.idle.appcore.types.WorkspaceMode
import ai.idle.appcore.types.WorkspaceOperation
import ai.idle.appcore.types.WorkspaceRequestState
import ai.idle.appcore.types.WorkspaceResponse
import ai.idle.appcore.types.WorkspaceResult
import ai.idle.appcore.types.WorkspaceSnapshot
import ai.idle.appcore.types.ConnectionStatus
import ai.idle.appcore.types.Context
import ai.idle.appcore.types.Reconciled
import ai.idle.appcore.types.SubscriptionAction
import ai.idle.appcore.types.SubscriptionEvent
import ai.idle.appcore.types.SubscriptionOperation
import ai.idle.appcore.types.SubscriptionResponse
import ai.idle.appcore.types.SubscriptionResult
import ai.idle.appcore.types.SessionAcknowledgement
import ai.idle.appcore.types.SessionAction
import ai.idle.appcore.types.SessionCapabilities
import ai.idle.appcore.types.SessionCapability
import ai.idle.appcore.types.SessionChange
import ai.idle.appcore.types.SessionChangeEvent
import ai.idle.appcore.types.SessionChanges
import ai.idle.appcore.types.SessionCompletion
import ai.idle.appcore.types.SessionContext
import ai.idle.appcore.types.SessionContributor
import ai.idle.appcore.types.SessionCursor
import ai.idle.appcore.types.SessionDelivery
import ai.idle.appcore.types.SessionEvent
import ai.idle.appcore.types.SessionGrant
import ai.idle.appcore.types.SessionGrantStatus
import ai.idle.appcore.types.SessionInfo
import ai.idle.appcore.types.SessionInputRef
import ai.idle.appcore.types.SessionInputState
import ai.idle.appcore.types.SessionInputUpdate
import ai.idle.appcore.types.SessionItemBinding
import ai.idle.appcore.types.SessionKind
import ai.idle.appcore.types.SessionMutation
import ai.idle.appcore.types.SessionMutationId
import ai.idle.appcore.types.SessionOperation
import ai.idle.appcore.types.SessionPermission
import ai.idle.appcore.types.SessionReceipt
import ai.idle.appcore.types.SessionRelationship
import ai.idle.appcore.types.SessionRequest
import ai.idle.appcore.types.SessionRequestKey
import ai.idle.appcore.types.SessionResponse
import ai.idle.appcore.types.SessionResult
import ai.idle.appcore.types.SessionRuntimeBinding
import ai.idle.appcore.types.SessionSnapshot

private fun view(core: AppCore): ViewModel = ViewModel.bincodeDeserialize(core.view())

private fun send(core: AppCore, event: Event): List<EffectRequest> =
    EffectBatch.bincodeDeserialize(core.processEvent(event.bincodeSerialize())).requests

private fun respond(core: AppCore, id: UInt, bytes: ByteArray): List<EffectRequest> =
    EffectBatch.bincodeDeserialize(core.handleResponse(id, bytes)).requests

private fun List<EffectRequest>.request(kind: EffectFfi): UInt = single { it.effect == kind }.id

private fun rejected(action: () -> Unit) {
    try {
        action()
    } catch (error: BindingError.Shell) {
        check(error.details.isNotBlank())
        return
    }
    error("Expected a typed BindingError.Shell exception from Rust")
}

private fun exercise(core: AppCore, other: AppCore) {
    val idle = view(core)
    check(!idle.initialized && idle.bootstrap == LoadState.Idle && idle.history.chain == null)
    check(core.protocolVersion() == 6u)
    check(view(core) == idle)
    rejected { core.processEvent(byteArrayOf()) }
    rejected { core.processEvent("invalid".encodeToByteArray()) }
    rejected { core.processEvent(Event.Start.bincodeSerialize() + byteArrayOf(0)) }
    // A host cannot forge the reducer's internal completion event.
    rejected { core.processEvent(byteArrayOf(1, 0, 0, 0, 1, 0, 0, 0)) }

    val effects = send(core, Event.Start)
    val id = effects.request(EffectFfi.HostInfo)
    val renderId = effects.first { it.effect == EffectFfi.Render }.id
    check(view(core) == ViewModel(initialized = true, bootstrap = LoadState.Loading, history = idle.history, workspace = idle.workspace, subscriptions = idle.subscriptions, sessions = idle.sessions))
    check(send(core, Event.Start).isEmpty())
    val info = HostInfo(name = "Kotlin/JVM host 🌍", version = "1.0")
    val success = HostInfoResponse.Ok(info).bincodeSerialize()
    rejected { other.handleResponse(id, success) }
    rejected { core.handleResponse(renderId, success) }
    rejected { core.handleResponse(UInt.MAX_VALUE, success) }
    rejected { core.handleResponse(id, byteArrayOf(0)) }
    rejected { core.handleResponse(id, success + byteArrayOf(0)) }
    check(respond(core, id, success).map { it.effect } == listOf(EffectFfi.Render))
    check(view(core) == ViewModel(initialized = true, bootstrap = LoadState.Ready(info), history = idle.history, workspace = idle.workspace, subscriptions = idle.subscriptions, sessions = idle.sessions))
    rejected { core.handleResponse(id, success) }
    check(view(other) == idle)

    val failedId = send(other, Event.Start).request(EffectFfi.HostInfo)
    val failure = EffectError(message = "Host unavailable 🌍")
    val failureBytes = HostInfoResponse.Err(failure).bincodeSerialize()
    check(respond(other, failedId, failureBytes).map { it.effect } == listOf(EffectFfi.Render))
    check(view(other).bootstrap == LoadState.Failed(failure))
    val retryId = send(other, Event.Bootstrap(BootstrapEvent.LOAD)).request(EffectFfi.HostInfo)
    check(retryId != failedId)
    rejected { other.handleResponse(failedId, success) }
    check(respond(other, retryId, success).map { it.effect } == listOf(EffectFfi.Render))
    check(view(other).bootstrap == LoadState.Ready(info))
    check(view(core).bootstrap == LoadState.Ready(info))
    val history = send(core, Event.History(HistoryEvent.Connect("chain"))).single { it.effect is EffectFfi.History }
    check((history.effect as EffectFfi.History).value.chain == "chain")
    check(view(core).history.paging.state == RequestState.Loading)
    val page = QueryResponse.Ok(QueryResult.History(HistoryPage(observations = emptyList(), nextAfter = null, scanned = 0u)))
    respond(core, history.id, page.bincodeSerialize())
    check(view(core).history.paging.state == RequestState.Ready)
    check(view(core).history.paging.exhausted)
    check(view(other).history.chain == null)
    val missingOperation = "a".repeat(64)
    val detailId = send(core, Event.History(HistoryEvent.LoadOperationDetails(missingOperation, false)))
        .request(EffectFfi.History(Query("chain", QueryAction.OperationDetails(missingOperation))))
    val details = OperationDetails(missingOperation, RecordLookupStatus.MISSING, null, emptyList(), emptyList(), null)
    respond(core, detailId, QueryResponse.Ok(QueryResult.OperationDetails(details)).bincodeSerialize())
    check(view(core).history.operationDetails.single().state == OperationDetailsState.Ready(details))
}

private fun workspaceSmoke(mode: WorkspaceMode) = AppCore().use { client ->
    val listId = send(client, Event.Workspace(WorkspaceEvent.Load)).request(EffectFfi.Workspace(WorkspaceOperation.List))
    val repo = RepositoryInfo(id = "repo", name = "Repository", remote = null)
    val info = WorkspaceInfo(id = "workspace", name = "Workspace 🌍", chain = "logical-chain",
        revision = ULong.MAX_VALUE, mode = mode, repositories = listOf(repo))
    respond(client, listId, WorkspaceResponse.Ok(WorkspaceResult.Directory(listOf(info))).bincodeSerialize())
    check(view(client).workspace.workspaces == listOf(info))
    val selected = send(client, Event.Workspace(WorkspaceEvent.SelectWorkspace("workspace")))
    val snapshotId = selected.request(EffectFfi.Workspace(WorkspaceOperation.Snapshot("workspace", mode)))
    val engine = selected.single { it.effect is EffectFfi.History }
    check((engine.effect as EffectFfi.History).value.chain == "logical-chain")
    respond(client, engine.id, QueryResponse.Ok(QueryResult.History(HistoryPage(emptyList(), null, 0u))).bincodeSerialize())
    val member = MemberInfo("alice", "Alice", ULong.MAX_VALUE, MemberRole.MEMBER, MemberStatus.ACTIVE)
    val snapshot = WorkspaceSnapshot(info, listOf(member), listOf("host"), listOf("provider"))
    val followup = respond(client, snapshotId, WorkspaceResponse.Ok(WorkspaceResult.Snapshot(snapshot)).bincodeSerialize())
    val presenceId = followup.request(EffectFfi.Workspace(WorkspaceOperation.Presence("workspace", mode)))
    val entry = PresenceEntry(connectionId = "connection", contributorId = "alice", status = PresenceStatus.ONLINE,
        repositoryId = "repo", branch = "main", file = "src/lib.rs", hostId = "host", summary = "Editing",
        observedAtMs = 100uL, validUntilMs = 200uL)
    val presence = PresenceSnapshot("workspace", 100uL, listOf(entry))
    respond(client, presenceId, WorkspaceResponse.Ok(WorkspaceResult.Presence(presence)).bincodeSerialize())
    check(view(client).workspace.members.single().presence == PresenceStatus.ONLINE)
    check(view(client).workspace.repositoryBinding?.chain == "logical-chain")
    send(client, Event.Workspace(WorkspaceEvent.Navigate(NavigationSection.AGENTRULES)))
    check(view(client).workspace.section == NavigationSection.AGENTRULES)
    send(client, Event.Workspace(WorkspaceEvent.Tick(200uL)))
    check(view(client).workspace.members.single().presence == PresenceStatus.UNKNOWN)
    val retryId = send(client, Event.Workspace(WorkspaceEvent.RefreshPresence))
        .request(EffectFfi.Workspace(WorkspaceOperation.Presence("workspace", mode)))
    val error = WorkspaceError(WorkspaceErrorKind.UNAVAILABLE, "Presence unavailable")
    respond(client, retryId, WorkspaceResponse.Err(error).bincodeSerialize())
    check(view(client).workspace.presenceState == WorkspaceRequestState.Failed(error))
    send(client, Event.Workspace(WorkspaceEvent.Disconnect))
    check(view(client).history.chain == null)
}

private fun subscriptionSmoke() = AppCore().use { client ->
    val context = Context("fixture", "workspace", "alice", "chain")
    val join = send(client, Event.Subscriptions(SubscriptionEvent.Connect(context)))
        .request(EffectFfi.Subscription(SubscriptionOperation(context, SubscriptionAction.Join)))
    val follow = respond(client, join, SubscriptionResponse.Ok(SubscriptionResult.Joined("connection")).bincodeSerialize())
    check(view(client).subscriptions.status == ConnectionStatus.RECONCILING)
    val read = follow.single { it.effect is EffectFfi.History }
    val watch = follow.request(EffectFfi.Subscription(SubscriptionOperation(context, SubscriptionAction.Watch("connection"))))
    val replacement = Reconciled(listOf(HistoryPage(emptyList(), null, 0u)), emptyList(), emptyList(), emptyList())
    respond(client, read.id, QueryResponse.Ok(QueryResult.Reconciled(replacement)).bincodeSerialize())
    check(view(client).subscriptions.status == ConnectionStatus.LIVE)
    check(view(client).history.reconciliation == RequestState.Ready)
    send(client, Event.Subscriptions(SubscriptionEvent.Reconnect))
    val before = view(client)
    respond(client, watch, SubscriptionResponse.Ok(SubscriptionResult.Changed).bincodeSerialize())
    check(view(client) == before)
    check(before.subscriptions.status == ConnectionStatus.CONNECTING)
}

private fun sessionSmoke(mode: WorkspaceMode) = AppCore().use { client ->
    val context = SessionContext("fixture", "workspace", "bob", "chain", mode)
    val load = send(client, Event.Sessions(SessionEvent.Connect(context)))
        .request(EffectFfi.Session(SessionOperation(context, SessionAction.Snapshot)))
    val actor = SessionContributor("bob", "peer", "bob-key")
    val cursor = SessionCursor("workspace", "bob", "stream", 100uL)
    val binding = SessionItemBinding("chain", "c".repeat(64))
    val session = SessionInfo("session", "alice", "Shared 🌍", SessionKind.RUNNER, 1uL,
        SessionRuntimeBinding("host", "runtime"), null, binding)
    val grant = SessionGrant("grant", "session", "bob", "alice", listOf(SessionPermission.OBSERVE, SessionPermission.SUBMITINPUT), 2000uL, 1uL, SessionGrantStatus.Active)
    val members = listOf(MemberInfo("alice", "Alice", 1uL, MemberRole.OWNER, MemberStatus.ACTIVE),
        MemberInfo("bob", "Bob", 1uL, MemberRole.MEMBER, MemberStatus.ACTIVE))
    val snapshot = SessionSnapshot(context, cursor, actor,
        SessionCapabilities(SessionCapability.UNAVAILABLE, SessionCapability.AVAILABLE, SessionCapability.AVAILABLE),
        members, listOf(session), listOf(grant), emptyList(), 1000uL)
    val follow = respond(client, load, SessionResponse.Ok(SessionResult.Snapshot(snapshot)).bincodeSerialize())
    val watch = follow.request(EffectFfi.Session(SessionOperation(context, SessionAction.Watch(cursor))))
    check(view(client).sessions.sessions.single().relationship == SessionRelationship.INVITED)
    send(client, Event.Sessions(SessionEvent.Select("session")))
    check(view(client).sessions.selectedHistory == binding)
    val mutationId = SessionMutationId("input", 2000uL)
    val attribution = SessionRequest("workspace", actor, mutationId)
    val submit = send(client, Event.Sessions(SessionEvent.Submit(mutationId, "Prompt 🌍\n")))
        .request(EffectFfi.Session(SessionOperation(context, SessionAction.Mutate(attribution, SessionMutation.Submit("session", "Prompt 🌍\n")))))
    val key = SessionRequestKey("workspace", "bob", "input")
    val receipt = SessionReceipt(key, 1000uL, 2500uL)
    respond(client, submit, SessionResponse.Ok(SessionResult.Acknowledged(SessionAcknowledgement.Received(receipt))).bincodeSerialize())
    check(view(client).sessions.prompts.single().runtime == null)
    check(view(client).sessions.prompts.single().contributor == actor)
    val delivery = SessionDelivery(1100uL, ULong.MAX_VALUE, 1150uL)
    val update = SessionInputUpdate(SessionInputRef("session", key), actor, "runtime", 4uL,
        SessionInputState.Completed(delivery, 1500uL, SessionCompletion.Succeeded))
    val through = SessionCursor("workspace", "bob", "stream", 101uL)
    val changes = SessionChanges(cursor, through, listOf(SessionChangeEvent(101uL, SessionChange.Input(update))), 1500uL)
    respond(client, watch, SessionResponse.Ok(SessionResult.Changes(changes)).bincodeSerialize())
    check(view(client).sessions.prompts.single().runtime == update)
    check(view(client).sessions.prompts.single().text == "Prompt 🌍\n")
    send(client, Event.Sessions(SessionEvent.Tick(2000uL)))
    check(view(client).sessions.selected == null)
    check(view(client).sessions.prompts.isEmpty())
}

fun main() {
    val core = AppCore()
    core.use { AppCore().use { other -> exercise(core, other) } }
    core.close() // Repeated release is safe; calls after release must be rejected.
    check(runCatching { core.view() }.exceptionOrNull() is IllegalStateException)
    workspaceSmoke(WorkspaceMode.STANDALONE)
    workspaceSmoke(WorkspaceMode.MANAGED)
    subscriptionSmoke()
    sessionSmoke(WorkspaceMode.STANDALONE)
    sessionSmoke(WorkspaceMode.MANAGED)
    println("Kotlin/JVM + JNI + Facet: event -> effect -> result -> typed view PASS")
    println("Kotlin/JVM operation records/content request -> result PASS")
    println("Kotlin/JVM workspace selection + members + presence in both modes PASS")
    println("Kotlin/JVM subscription join + snapshot + stale watch PASS")
    println("Kotlin/JVM session attribution + receipt + runtime completion + expiry in both modes PASS")
}
