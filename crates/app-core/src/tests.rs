use crate::{
    Core, Effect, Event, Shell, ShellError, ViewModel, bootstrap,
    effects::{EffectFfi, HostInfo, HostInfoOperation},
    module::{EffectError, LoadState},
    shell::{EffectRequest, ShellFormat},
};

use crux_core::bridge::FfiFormat;

const START: &[u8] = b"\0\0\0\0";
const RETRY: &[u8] = b"\x01\0\0\0\0\0\0\0";
const SUCCESS: &[u8] = b"\0\0\0\0\x09\0\0\0\0\0\0\0Test host\x03\0\0\0\0\0\0\x001.0";
const FAILURE: &[u8] = b"\x01\0\0\0\x10\0\0\0\0\0\0\0Host unavailable";

fn host_info() -> HostInfo {
    HostInfo {
        name: "Test host".into(),
        version: "1.0".into(),
    }
}

fn host_request(effects: Vec<Effect>) -> crux_core::Request<HostInfoOperation> {
    let mut requests = effects.into_iter().filter_map(|effect| match effect {
        Effect::HostInfo(request) => Some(request),
        Effect::Render(_)
        | Effect::History(_)
        | Effect::Workspace(_)
        | Effect::Subscription(_)
        | Effect::Session(_)
        | Effect::Projection(_)
        | Effect::Resource(_)
        | Effect::Configuration(_) => None,
    });
    let request = requests.next().expect("one host request");
    assert!(
        requests.next().is_none(),
        "must issue only one host request"
    );
    request
}

fn requests(bytes: &[u8]) -> Vec<EffectRequest> {
    ShellFormat::deserialize(bytes).expect("a valid request batch")
}

fn request_id(requests: &[EffectRequest]) -> u32 {
    requests
        .iter()
        .find(|request| request.effect == EffectFfi::HostInfo)
        .expect("one host operation")
        .id
}

fn view(shell: &Shell) -> ViewModel {
    ShellFormat::deserialize(&shell.view().expect("view bytes")).expect("typed view")
}

#[test]
fn client_event_host_result_and_typed_view() {
    let core = Core::new();
    assert_eq!(core.view(), ViewModel::default(), "new clients start empty");
    let effects = core.process_event(Event::Start);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Render(_))),
        "loading must render"
    );
    assert_eq!(
        core.view().bootstrap,
        LoadState::Loading,
        "host work is pending"
    );
    assert!(
        core.view().initialized,
        "start still initializes the client"
    );
    let mut request = host_request(effects);
    let follow_up = core
        .resolve(&mut request, Ok(host_info()))
        .expect("resolve host result");
    assert!(
        matches!(follow_up.as_slice(), [Effect::Render(_)]),
        "completion requests presentation"
    );
    assert_eq!(
        core.view().bootstrap,
        LoadState::Ready(host_info()),
        "the continuation updates the domain view"
    );
}

#[test]
fn failure_is_visible_and_retry_can_succeed() {
    let core = Core::new();
    let mut first = host_request(core.process_event(Event::Start));
    let error = EffectError {
        message: "Host unavailable".into(),
    };
    let _effects = core
        .resolve(&mut first, Err(error.clone()))
        .expect("host failure is a valid result");
    assert_eq!(
        core.view().bootstrap,
        LoadState::Failed(error),
        "show the host failure"
    );
    let mut retry = host_request(core.process_event(Event::Bootstrap(bootstrap::Event::Load)));
    assert_eq!(
        core.view().bootstrap,
        LoadState::Loading,
        "retry replaces the failure"
    );
    let _effects = core
        .resolve(&mut retry, Ok(host_info()))
        .expect("resolve retry");
    assert_eq!(
        core.view().bootstrap,
        LoadState::Ready(host_info()),
        "retry can recover"
    );
}

#[test]
fn repeated_loads_do_not_duplicate_pending_or_completed_work() {
    let core = Core::new();
    let mut request = host_request(core.process_event(Event::Start));
    for event in [Event::Start, Event::Bootstrap(bootstrap::Event::Load)] {
        assert!(
            core.process_event(event).is_empty(),
            "an in-flight load is not duplicated"
        );
    }
    let _effects = core
        .resolve(&mut request, Ok(host_info()))
        .expect("resolve request");
    assert!(
        core.process_event(Event::Start).is_empty(),
        "start is idempotent"
    );
    assert!(
        core.process_event(Event::Bootstrap(bootstrap::Event::Load))
            .is_empty(),
        "ready information remains cached"
    );
}

#[test]
fn module_actions_can_precede_start_without_losing_initialization_render() {
    let core = Core::new();
    let _request = host_request(core.process_event(Event::Bootstrap(bootstrap::Event::Load)));
    assert!(
        !core.view().initialized,
        "module load is independent of root initialization"
    );
    assert!(
        matches!(
            core.process_event(Event::Start).as_slice(),
            [Effect::Render(_)]
        ),
        "root change must render even during a load"
    );
    assert!(
        core.view().initialized,
        "root initializes during an existing load"
    );
}

#[test]
fn binary_shell_round_trip_matches_the_rust_view() {
    let shell = Shell::new();
    assert_eq!(
        view(&shell),
        ViewModel::default(),
        "the binary view matches Rust defaults"
    );
    let effects = requests(&shell.process_event(START).expect("start event"));
    assert_eq!(
        view(&shell).bootstrap,
        LoadState::Loading,
        "effect waits for the host"
    );
    let effects = requests(
        &shell
            .handle_response(request_id(&effects), SUCCESS)
            .expect("response"),
    );
    assert_eq!(effects.len(), 1, "response requests one render");
    assert_eq!(
        effects.first().map(|request| request.effect.clone()),
        Some(EffectFfi::Render),
        "render is a notification"
    );
    assert_eq!(
        view(&shell),
        ViewModel {
            initialized: true,
            bootstrap: LoadState::Ready(host_info()),
            ..ViewModel::default()
        },
        "the wire view carries typed host information"
    );
}

#[test]
fn malformed_events_and_forged_completions_leave_state_unchanged() {
    let shell = Shell::new();
    for event in [
        b"legacy JSON".as_slice(),
        b"\xff\xff\xff\xff",
        b"\x01\0\0\0\x01\0\0\0", // Internal Completed variant.
        b"\0\0\0\0\x01",         // Trailing bytes after Start.
    ] {
        assert!(
            shell.process_event(event).is_err(),
            "invalid client input must be rejected"
        );
        assert_eq!(
            view(&shell),
            ViewModel::default(),
            "rejected input must not change state"
        );
    }
}

#[test]
fn malformed_results_can_be_corrected_without_losing_the_request() {
    let shell = Shell::new();
    let effects = requests(&shell.process_event(START).expect("start event"));
    let id = request_id(&effects);
    for response in [
        b"bad bytes".as_slice(),
        b"\0\0\0\0",   // Ok without HostInfo.
        b"\x01\0\0\0", // Err without EffectError.
    ] {
        assert!(
            matches!(
                shell.handle_response(id, response),
                Err(ShellError::InvalidResponse(_))
            ),
            "bad results must be rejected before the continuation is consumed"
        );
        assert_eq!(
            view(&shell).bootstrap,
            LoadState::Loading,
            "invalid results do not mutate state"
        );
    }
    let _effects = shell
        .handle_response(id, SUCCESS)
        .expect("corrected result");
    assert_eq!(
        view(&shell).bootstrap,
        LoadState::Ready(host_info()),
        "the original request is still resolvable"
    );
}

#[test]
fn unknown_notification_and_duplicate_response_ids_are_rejected() {
    let shell = Shell::new();
    let effects = requests(&shell.process_event(START).expect("start event"));
    let notification = effects
        .iter()
        .find(|request| request.effect == EffectFfi::Render)
        .expect("render request");
    for id in [u32::MAX, notification.id] {
        assert!(
            shell.handle_response(id, SUCCESS).is_err(),
            "only outstanding request IDs can resolve"
        );
        assert_eq!(
            view(&shell).bootstrap,
            LoadState::Loading,
            "bad IDs do not update the view"
        );
    }
    let id = request_id(&effects);
    let _effects = shell.handle_response(id, SUCCESS).expect("valid response");
    assert!(
        shell.handle_response(id, FAILURE).is_err(),
        "completed IDs cannot be reused"
    );
    assert_eq!(
        view(&shell).bootstrap,
        LoadState::Ready(host_info()),
        "duplicates cannot overwrite success"
    );
}

#[test]
fn retry_does_not_reuse_a_completed_request_id() {
    let shell = Shell::new();
    let first = request_id(&requests(&shell.process_event(START).expect("start")));
    let _effects = shell
        .handle_response(first, FAILURE)
        .expect("failure response");
    assert!(
        matches!(view(&shell).bootstrap, LoadState::Failed(_)),
        "error is presentable"
    );
    let second = request_id(&requests(&shell.process_event(RETRY).expect("retry")));
    assert_ne!(first, second, "a retry must receive a new ID");
    assert!(
        shell.handle_response(first, SUCCESS).is_err(),
        "a late response cannot satisfy the retry"
    );
    assert_eq!(
        view(&shell).bootstrap,
        LoadState::Loading,
        "the current request remains pending"
    );
    let _effects = shell
        .handle_response(second, SUCCESS)
        .expect("retry response");
    assert_eq!(
        view(&shell).bootstrap,
        LoadState::Ready(host_info()),
        "only the current request updates state"
    );
}

#[test]
fn clients_have_independent_models_and_registries() {
    let first = Shell::new();
    let second = Shell::new();
    let id = request_id(&requests(&first.process_event(START).expect("start first")));
    assert!(
        second.handle_response(id, SUCCESS).is_err(),
        "the other client has no pending request"
    );
    let _effects = first.handle_response(id, SUCCESS).expect("first response");
    assert_eq!(
        view(&second),
        ViewModel::default(),
        "the other model remains untouched"
    );
    assert_eq!(
        view(&first).bootstrap,
        LoadState::Ready(host_info()),
        "first model changes independently"
    );
}

#[test]
fn concurrent_native_calls_issue_one_bootstrap_operation() {
    let shell = Shell::new();
    let batches = std::thread::scope(|scope| {
        let calls: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| shell.process_event(START)))
            .collect();
        calls
            .into_iter()
            .flat_map(|call| requests(&call.join().expect("join call").expect("process event")))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        batches
            .iter()
            .filter(|request| request.effect == EffectFfi::HostInfo)
            .count(),
        1,
        "calls share one atomic event/effect boundary"
    );
    let _effects = shell
        .handle_response(request_id(&batches), SUCCESS)
        .expect("resolve concurrent start");
    assert_eq!(
        view(&shell).bootstrap,
        LoadState::Ready(host_info()),
        "the single continuation is preserved"
    );
}

#[test]
fn protocol_v4_binary_layout_matches_the_public_types() {
    use crate::effects::{HostInfoResponse, HostInfoResult};
    use crate::shell::{EffectBatch, PROTOCOL_VERSION};

    fn encode(value: &impl serde::Serialize) -> Vec<u8> {
        let mut bytes = Vec::new();
        ShellFormat::serialize(&mut bytes, value).expect("encode payload");
        bytes
    }

    assert_eq!(
        PROTOCOL_VERSION, 11,
        "workspace navigation extends the binary shell protocol"
    );
    assert_eq!(encode(&Event::Start), START, "stable Start discriminant");
    assert_eq!(
        encode(&Event::Bootstrap(bootstrap::Event::Load)),
        RETRY,
        "stable nested client event"
    );
    for (result, expected) in [
        (Ok(host_info()), SUCCESS),
        (
            Err(EffectError {
                message: "Host unavailable".into(),
            }),
            FAILURE,
        ),
    ] {
        assert_eq!(encode(&result), expected, "stable host result encoding");
        let response = HostInfoResponse::from(result.clone());
        assert_eq!(
            encode(&response),
            expected,
            "generated result wrapper matches Crux"
        );
        let decoded: HostInfoResult = ShellFormat::deserialize(expected).expect("decode result");
        assert_eq!(decoded, result, "host result decodes without loss");
    }
    assert_eq!(
        ShellFormat::deserialize::<ViewModel>(&encode(&ViewModel::default()))
            .expect("initial view"),
        ViewModel::default(),
        "full history view round-trips"
    );
    let shell = Shell::new();
    let bytes = shell.process_event(START).expect("start");
    let batch: EffectBatch = ShellFormat::deserialize(&bytes).expect("typed batch");
    assert_eq!(
        encode(&batch),
        bytes,
        "generated batch wrapper matches Crux"
    );
    assert_eq!(
        encode(&batch.requests),
        bytes,
        "Crux serializes a request vector"
    );
}
