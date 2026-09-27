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
import ai.idle.appcore.types.LoadState
import ai.idle.appcore.types.ViewModel

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
    val idle = ViewModel(initialized = false, bootstrap = LoadState.Idle)
    check(core.protocolVersion() == 2u)
    check(view(core) == idle)
    rejected { core.processEvent(byteArrayOf()) }
    rejected { core.processEvent("invalid".encodeToByteArray()) }
    rejected { core.processEvent(Event.Start.bincodeSerialize() + byteArrayOf(0)) }
    // A host cannot forge the reducer's internal completion event.
    rejected { core.processEvent(byteArrayOf(1, 0, 0, 0, 1, 0, 0, 0)) }

    val effects = send(core, Event.Start)
    val id = effects.request(EffectFfi.HOSTINFO)
    val renderId = effects.first { it.effect == EffectFfi.RENDER }.id
    check(view(core) == ViewModel(initialized = true, bootstrap = LoadState.Loading))
    check(send(core, Event.Start).isEmpty())
    val info = HostInfo(name = "Kotlin/JVM host 🌍", version = "1.0")
    val success = HostInfoResponse.Ok(info).bincodeSerialize()
    rejected { other.handleResponse(id, success) }
    rejected { core.handleResponse(renderId, success) }
    rejected { core.handleResponse(UInt.MAX_VALUE, success) }
    rejected { core.handleResponse(id, byteArrayOf(0)) }
    rejected { core.handleResponse(id, success + byteArrayOf(0)) }
    check(respond(core, id, success).map { it.effect } == listOf(EffectFfi.RENDER))
    check(view(core) == ViewModel(initialized = true, bootstrap = LoadState.Ready(info)))
    rejected { core.handleResponse(id, success) }
    check(view(other) == idle)

    val failedId = send(other, Event.Start).request(EffectFfi.HOSTINFO)
    val failure = EffectError(message = "Host unavailable 🌍")
    val failureBytes = HostInfoResponse.Err(failure).bincodeSerialize()
    check(respond(other, failedId, failureBytes).map { it.effect } == listOf(EffectFfi.RENDER))
    check(view(other).bootstrap == LoadState.Failed(failure))
    val retryId = send(other, Event.Bootstrap(BootstrapEvent.LOAD)).request(EffectFfi.HOSTINFO)
    check(retryId != failedId)
    rejected { other.handleResponse(failedId, success) }
    check(respond(other, retryId, success).map { it.effect } == listOf(EffectFfi.RENDER))
    check(view(other).bootstrap == LoadState.Ready(info))
    check(view(core).bootstrap == LoadState.Ready(info))
}

fun main() {
    val core = AppCore()
    core.use { AppCore().use { other -> exercise(core, other) } }
    core.close() // Repeated release is safe; calls after release must be rejected.
    check(runCatching { core.view() }.exceptionOrNull() is IllegalStateException)
    println("Kotlin/JVM + JNI + Facet: event -> effect -> result -> typed view PASS")
}
