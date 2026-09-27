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
let idle = ViewModel(initialized: false, bootstrap: .idle)
check(core.protocolVersion() == 2)
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
check(try view(core) == ViewModel(initialized: true, bootstrap: .ready(info)))
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
