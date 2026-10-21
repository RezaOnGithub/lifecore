# Durable application receipts

Status: backlogged contract for the application service, retained for a later
round; nothing here is a prerequisite for the web application. The
[optional token](../FILES.md#optional-structured-plan-identity) does not claim
to implement these recovery requirements. Plans participating in retry
recovery would carry `plan_id`; omitted tokens carry no retry identity
guarantee.

## 1. Requirements

- Persist the frozen plan identity/content, actual base, check context, and
  assigned operation IDs in the same atomic publication as the graph. A
  workspace-only journal written afterward cannot close the crash window.
- Retrying an identical completed plan returns its receipt without appending
  again. Reusing its identity with different content is an error.
- Verify identity/content and look up the receipt before checking the original
  expected base, since a successful application has itself advanced that base.
- Recover uncertain acknowledgment and any
  [collection writeback](COLLECTIONS.md#8-source-edits-and-file-writeback)
  without repeating graph appends. Receipts are auxiliary persistence metadata,
  not graph entries.

The future design must cover empty plans as well as plans that change the graph.
Replay-verified acknowledgment records nothing about prior use. An empty
suggestion can acknowledge a finding without changing graph revision; restoring
and reapplying it may acknowledge again. A dismissal record is not a receipt.
Future recovery must include the combined graph/dismissal result and avoid
silently reapplying either effect after a later restore.
Receipt retention, encoding, and storage layout remain decisions for that work.

## 2. Acceptance scenarios

| Scenario | Required outcome when receipt recovery is implemented |
| --- | --- |
| Crash occurs after commit but before client acknowledgment | Recover the operation receipt; do not append twice. |
| A completed plan is retried after its base has advanced | Verify plan identity/content and return the receipt before stale-base rejection. |
| Plan identity is reused with changed content | Reject the conflicting identity, even if the earlier plan committed. |
| An empty plan is retried | Recognize its completed application through its receipt even if the graph/suite state did not change. |
