use crux_core::Request;

use super::{
    ConnectionStatus, Context, Event, SubscriptionAction, SubscriptionOperation, SubscriptionResult,
};
use crate::{Core, Effect, Event as RootEvent, history};

#[derive(Default)]
struct Work {
    connections: Vec<Request<SubscriptionOperation>>,
    reads: Vec<Request<history::Query>>,
}

fn work(effects: Vec<Effect>) -> Work {
    let mut work = Work::default();
    for effect in effects {
        match effect {
            Effect::Subscription(request) => work.connections.push(*request),
            Effect::History(request) => work.reads.push(*request),
            Effect::Render(_) | Effect::HostInfo(_) | Effect::Workspace(_) | Effect::Session(_) => {
            }
        }
    }
    work
}

fn context(provider: &str) -> Context {
    Context {
        provider: provider.into(),
        workspace: "workspace".into(),
        contributor: "person".into(),
        chain: "chain".into(),
    }
}

fn connect(core: &Core, provider: &str) -> Request<SubscriptionOperation> {
    let mut work =
        work(core.process_event(RootEvent::Subscriptions(Event::Connect(context(provider)))));
    assert!(
        work.reads.is_empty(),
        "join must register notifications before reading"
    );
    work.connections.pop().expect("join request")
}

fn joined(core: &Core, request: &mut Request<SubscriptionOperation>, connection: &str) -> Work {
    work(
        core.resolve(
            request,
            Ok(SubscriptionResult::Joined {
                connection: connection.into(),
            }),
        )
        .expect("join continuation"),
    )
}

fn snapshot(core: &Core, request: &mut Request<history::Query>) -> Work {
    let input = if let history::QueryAction::Reconcile(input) = &request.operation.action {
        Some(input)
    } else {
        None
    }
    .expect("replacement read expected");
    let result = history::Reconciled {
        history: vec![history::HistoryPage::default()],
        search: if input.text.is_empty() {
            Vec::new()
        } else {
            vec![history::SearchPage::default()]
        },
        items: input
            .items
            .iter()
            .map(|item| history::ItemSnapshot {
                item: item.item.clone(),
                pages: vec![history::HistoryPage::default()],
            })
            .collect(),
        details: Vec::new(),
    };
    work(
        core.resolve(
            request,
            Ok(history::QueryResult::Reconciled(Box::new(result))),
        )
        .expect("snapshot continuation"),
    )
}

#[test]
fn connection_waits_for_snapshot_and_changes_during_recovery_are_not_lost() {
    let core = Core::new();
    let mut join = connect(&core, "standalone");
    let mut started = joined(&core, &mut join, "connection-1");
    assert_eq!(
        core.view().subscriptions.status,
        ConnectionStatus::Reconciling,
        "join alone is not live"
    );
    let mut read = started.reads.pop().expect("replacement read");
    let mut watch = started.connections.pop().expect("buffered watch");
    let mut changed = work(
        core.resolve(&mut watch, Ok(SubscriptionResult::Changed))
            .expect("change during snapshot"),
    );
    assert!(
        changed.reads.is_empty(),
        "invalidations coalesce behind the pending read"
    );
    let mut follow_up = snapshot(&core, &mut read);
    assert_eq!(
        core.view().subscriptions.status,
        ConnectionStatus::Reconciling,
        "dirty snapshot is not live"
    );
    let mut retry = follow_up.reads.pop().expect("dirty snapshot repeats");
    assert!(
        snapshot(&core, &mut retry).reads.is_empty(),
        "one replacement catches up"
    );
    assert_eq!(
        core.view().subscriptions.status,
        ConnectionStatus::Live,
        "reconciled before ready"
    );
    let mut watch = changed.connections.pop().expect("next watch");
    let dropped = work(
        core.resolve(&mut watch, Ok(SubscriptionResult::Closed))
            .expect("transport loss"),
    );
    assert_eq!(
        core.view().subscriptions.status,
        ConnectionStatus::Waiting,
        "automatic retry state"
    );
    let mut timer = dropped
        .connections
        .into_iter()
        .find(|request| matches!(request.operation.action, SubscriptionAction::Wait { .. }))
        .expect("retry timer");
    let mut retry = work(
        core.resolve(&mut timer, Ok(SubscriptionResult::Elapsed))
            .expect("timer elapsed"),
    );
    let mut join = retry.connections.pop().expect("new join");
    assert_eq!(
        join.operation.action,
        SubscriptionAction::Join,
        "no operation cursor used on reconnect"
    );
    let mut started = joined(&core, &mut join, "connection-2");
    let mut read = started.reads.pop().expect("new snapshot");
    let _work = snapshot(&core, &mut read);
    assert_eq!(
        core.view().subscriptions.status,
        ConnectionStatus::Live,
        "new connection reconciles again"
    );
}

#[test]
fn a_b_a_discards_old_reads_joins_watch_results_and_retry_timers() {
    let core = Core::new();
    let mut old_join = connect(&core, "a");
    let mut other_join = connect(&core, "b");
    let mut other = joined(&core, &mut other_join, "b-connection");
    let mut old_read = other.reads.pop().expect("b read");
    let mut old_watch = other.connections.pop().expect("b watch");
    let _new_join = connect(&core, "a");
    let before = core.view();
    let cleanup = joined(&core, &mut old_join, "retired-a-connection");
    assert!(cleanup.reads.is_empty(), "late join cannot start a read");
    assert!(
        matches!(cleanup.connections.first().map(|r| &r.operation.action), Some(SubscriptionAction::Leave { connection }) if connection == "retired-a-connection"),
        "retired join is released"
    );
    let _ignored = snapshot(&core, &mut old_read);
    let ignored = work(
        core.resolve(&mut old_watch, Ok(SubscriptionResult::Changed))
            .expect("old watch"),
    );
    assert!(
        ignored.reads.is_empty() && ignored.connections.is_empty(),
        "old watch cannot restart work"
    );
    assert_eq!(
        core.view(),
        before,
        "old context cannot mutate current state"
    );

    let mut join = connect(&core, "timer-context");
    let failed = work(
        core.resolve(
            &mut join,
            Err(super::SubscriptionError {
                kind: super::SubscriptionErrorKind::Transport,
                message: "offline".into(),
            }),
        )
        .expect("failed join"),
    );
    let mut timer = failed.connections.into_iter().next().expect("retry timer");
    let _new = connect(&core, "new-context");
    let before = core.view();
    let result = work(
        core.resolve(&mut timer, Ok(SubscriptionResult::Elapsed))
            .expect("retired timer"),
    );
    assert!(result.connections.is_empty(), "retired timer cannot rejoin");
    assert_eq!(core.view(), before, "timer context is discarded");
}
