# Repository views and recorded sessions

Shell protocol **13** adds `Event::Repository`, `Effect::Repository` and
`view.repository`. Regenerate native codecs together. Repository DTOs are shared
with `idle-protocol`; the reducer owns read correlation, context retirement,
selection and history navigation. Hosts own paths, credentials and persistence.

A `RepositoryContext` combines the authorized subscription context with one exact
repository ID. Replacements must match workspace, chain and repository and pass
shared snapshot validation. Provider, contributor, repository or connection
changes retire previous requests. Suspend retains stale data; disconnect clears
it. Late results cannot replace a new binding.

`Read` explicitly rechecks GitHub, `Poll` permits a recent GitHub read while
refreshing local records, and `SignIn` asks the host to obtain repository access.
Requests during an outstanding read coalesce; explicit refresh takes precedence.
Source reports distinguish complete, partial, unavailable and inapplicable reads.
Automatic cache reuse preserves the original source check time.

Recorded sessions are full logical history items, separate from live runtime
sessions. Selecting one emits the exact session filter and history item. Inspect
accepts only a source supplied for that session in the current snapshot. The host
receives `Remember` for each local selection and returns a saved selection with
reads. A valid saved choice initializes a reopened view once; later responses
cannot overwrite a newer local choice. Partial or unavailable session lists keep
the selection, while a complete replacement removes a session that is no longer
present. Navigation to Activity clears the session filter; returning to Sessions
restores the selected session. A delayed read can restore the saved preference
while Activity is open, but applies its history filter only in Sessions.
Projection source inspection opens unfiltered
Activity before selecting the exact source record.

The standalone host uses `idle-repository` for Git/GitHub interpretation and native
history queries for exact record checks. Shared UI components show repository
facts, distinct Git/GitHub/Idle user lists and recorded sessions. Resource control,
agent execution and managed services keep their own capability gates.
