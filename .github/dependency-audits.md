# Dependency audit ownership

The maintainer merging a PR owns review of its `deny (advisories)` and
`audit-frontend` results. These jobs remain advisory because a newly published
vulnerability can affect an otherwise unrelated change. License, source and banned
dependency checks remain blocking.

For a new finding, the maintainer records the advisory ID, affected dependency,
reachability in Kelvo, remediation owner and target date in a tracking issue before
merging. The PR author owns findings introduced by that PR; an existing dependency
finding is assigned to the release maintainer. An already tracked finding links to
its issue rather than creating a duplicate. Credential exposure or a reachable
critical vulnerability blocks release until fixed or explicitly mitigated.

Before each release, the release maintainer reruns both audits, reviews open
findings and records remaining mitigations in the release checklist. Prefer updating
the dependency. A temporary ignore in `deny.toml` needs a reason, an issue link and
an expiry date. Do not turn off an advisory job to make a run appear green.

No scheduled audit is configured; audit results are refreshed on PRs, main pushes,
manual CI runs and the release review.
