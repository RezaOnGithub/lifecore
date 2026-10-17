import { useState, useSyncExternalStore } from "react";
import type { AnalyzerConfig, Description } from "../contract.ts";
import { useData } from "../data.tsx";
import { useDraft } from "../draft/context.tsx";
import { countWords } from "../draft/findings.ts";
import { type Evaluation, evaluationParams } from "../draft/model.ts";
import { FailureView } from "../failure.tsx";
import { sameId } from "../model/labels.ts";
import { counted } from "../model/words.ts";
import type { RpcClient } from "../rpc.ts";
import {
  type AddOutcome,
  AnalyzerAdds,
  addCounts,
  boundaryWords,
  positionsOf,
  type Sent,
  suiteHolds,
} from "./analyzerAdd.ts";
import { EntryChip, FindingRow, NOT_AT_HEAD } from "./FindingRow.tsx";
import { EvaluationChoice } from "./TrayHeader.tsx";

const stores = new WeakMap<RpcClient, AnalyzerAdds>();

function useAnalyzerAdds(): AnalyzerAdds {
  const { client } = useData();
  let store = stores.get(client);
  if (store === undefined) {
    store = new AnalyzerAdds((method, params) => client.call(method, params));
    stores.set(client, store);
  }
  useSyncExternalStore(store.subscribe, store.version);
  return store;
}

// The configuration string exactly as stored or sent; an empty one says so.
export function Verbatim({ text }: { text: string }) {
  return text === "" ? (
    <span className="muted">(empty string)</span>
  ) : (
    <code className="verbatim">{text}</code>
  );
}

function SuiteList({
  suite,
  testId,
}: {
  suite: readonly AnalyzerConfig[];
  testId: string;
}) {
  return (
    <ol className="suite-list" data-testid={testId}>
      {suite.map((analyzer, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: position is the identity
        <li key={index}>
          {analyzer.name} <Verbatim text={analyzer.configuration} />
        </li>
      ))}
    </ol>
  );
}

function evaluationWords(evaluation: Evaluation): string {
  switch (evaluation.kind) {
    case "none":
      return "none";
    case "now":
      return "now, captured once by the host before the replay";
    case "at":
      return evaluation.time;
  }
}

// The first registered analyzer the suite does not configure yet, else the
// first one; null when the host registers none.
function defaultName(description: Description): string | null {
  const configured = new Set(description.suite.map((config) => config.name));
  return (
    description.registered.find((name) => !configured.has(name)) ??
    description.registered[0] ??
    null
  );
}

// Appending to the suite is deliberate and permanent, so "Add…" only opens
// this; the request leaves on its button, once.
function Confirmation({
  description,
  name,
  configuration,
  evaluation,
  onConfirm,
  onCancel,
}: {
  description: Description;
  name: string;
  configuration: string;
  evaluation: Evaluation;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { state } = useDraft();
  const { suite } = description;
  const operations = state.proposal.entries.length;
  const last = suite.at(-1);
  const entries = description.view.end_exclusive;
  return (
    <div className="banner confirm-add" data-testid="add-confirmation">
      <strong>Append this analyzer to the suite?</strong>
      <ul>
        <li>
          analyzer <strong>{name}</strong>
        </li>
        <li>
          configuration <Verbatim text={configuration} />, stored exactly as
          typed: the string is part of the identity of every finding it emits
        </li>
        <li data-testid="add-position">
          position {suite.length + 1} of the suite as last read
          {last === undefined ? ", its first analyzer" : `, after ${last.name}`}
        </li>
        <li>evaluation time: {evaluationWords(evaluation)}</li>
      </ul>
      <p className="note">
        The whole history, {counted(entries, "entry")}, is replayed at every
        prefix under the extended suite. An inconsistency anywhere rejects the
        addition, and nothing is stored.
      </p>
      <p className="note">
        There is no removal or reconfiguration: once added, it stays in the
        suite.
      </p>
      <p className="note" data-testid="add-plan-note">
        Success changes the graph revision, so every outstanding plan ID becomes
        stale
        {operations > 0
          ? `: your draft of ${counted(operations, "operation")} will need Replan.`
          : ", and a draft with operations would need Replan."}
      </p>
      <div className="row">
        <button type="button" onClick={onConfirm}>
          Add {name}
        </button>
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

function Outcome({
  outcome,
  onRetry,
}: {
  outcome: Exclude<AddOutcome, { kind: "sending" }>;
  onRetry: (sent: Sent) => void;
}) {
  const store = useAnalyzerAdds();
  const { bumpRefresh } = useData();
  const { state } = useDraft();
  const { sent } = outcome;
  const { name, configuration } = sent.params;
  const dismiss = (
    <button type="button" onClick={() => store.dismiss()}>
      Dismiss
    </button>
  );
  switch (outcome.kind) {
    case "added": {
      const { added } = outcome;
      const counts = addCounts(added.report, sent);
      const time = added.report.analysis.inputs.evaluation_time;
      return (
        <div className="banner applied" role="status" data-testid="add-added">
          Added <strong>{name}</strong> <Verbatim text={configuration} /> as
          analyzer {added.suite.length} of the suite; graph revision{" "}
          <code>{added.graph_revision}</code>. Outstanding plan IDs are stale
          now
          {state.proposal.entries.length > 0 &&
            "; Replan the draft before checking it again"}
          .
          <SuiteList suite={added.suite} testId="added-suite" />
          <p className="note" data-testid="add-findings">
            Findings of the replay: {countWords(counts.total)}; from {name}:{" "}
            {countWords(counts.added)}.
            {time !== null && ` Evaluated at ${time}.`}
          </p>
          {dismiss}
        </div>
      );
    }
    case "rejected": {
      const { report } = outcome;
      const nodes = outcome.nodes ?? new Map();
      const boundary = report.analysis.view.end_exclusive;
      const inconsistencies = report.analysis.observations.filter(
        (observation) => observation.diagnostic.kind === "inconsistency",
      );
      const { total } = addCounts(report, sent);
      return (
        <div className="banner problem" role="alert" data-testid="add-rejected">
          <strong>Not added.</strong> Replaying the history under the extended
          suite emitted {`${counted(inconsistencies.length, "inconsistency")};`}{" "}
          nothing was stored.
          <ul className="findings" data-testid="add-inconsistencies">
            {inconsistencies.map((observation, index) => (
              <FindingRow
                // biome-ignore lint/suspicious/noArrayIndexKey: one report's observations are fixed
                key={index}
                observation={observation}
                boundary={boundary}
                ended={NOT_AT_HEAD}
                focus={observation.diagnostic.focus}
                subjects={observation.diagnostic.subjects}
                same={sameId}
                chip={(id) => <EntryChip id={id} nodes={nodes} />}
              />
            ))}
          </ul>
          <p className="note" data-testid="add-other-findings">
            Other findings of the replay, not listed:{" "}
            {counted(total.warning, "warning")},{" "}
            {counted(total.suggestion, "suggestion")}.
          </p>
          {dismiss}
        </div>
      );
    }
    case "analyzer_failure": {
      const { failure } = outcome;
      return (
        <div className="banner problem" role="alert" data-testid="add-failure">
          <strong>Not added: analyzer failure.</strong> Nothing was stored.
          {failure !== null && (
            <p className="note" data-testid="add-failing">
              {failure.analyzer.name} {failure.analyzer.version}{" "}
              <Verbatim text={failure.analyzer.configuration} /> failed{" "}
              {boundaryWords(failure.evaluated_view)}: {failure.message}
            </p>
          )}
          <p className="note muted">{outcome.message}</p>
          {dismiss}
        </div>
      );
    }
    case "busy":
      return (
        <div className="banner busy" role="alert" data-testid="add-busy">
          <strong>Not added: the database is busy.</strong> {outcome.message}{" "}
          Nothing was stored.
          <div className="row">
            <button type="button" onClick={() => onRetry(sent)}>
              Try again
            </button>
            {dismiss}
          </div>
        </div>
      );
    case "failed":
      return (
        <div className="banner problem" data-testid="add-failed">
          <strong>Not added;</strong> nothing was stored.
          <FailureView failure={outcome.failure} compact />
          {dismiss}
        </div>
      );
    case "uncertain": {
      const reported = outcome.outcome;
      const { reread } = outcome;
      return (
        <div
          className="banner uncertain"
          role="alert"
          data-testid="add-uncertain"
        >
          <strong>The analyzer may have been added.</strong>{" "}
          <code>{outcome.error}</code>: {outcome.message}
          {reported !== null && (
            <>
              <p className="note">
                The server reports this outcome: graph revision{" "}
                <code>{reported.graph_revision}</code>, suite:
              </p>
              <SuiteList suite={reported.suite} testId="reported-suite" />
            </>
          )}
          <p className="note" data-testid="add-verdict">
            {reread?.status !== "read"
              ? "Refresh re-reads the suite; Add stays off until then, so the analyzer cannot be appended twice."
              : suiteHolds(reread.suite, sent)
                ? `The suite read since holds it, as analyzer ${positionsOf(reread.suite, sent).at(-1)}.`
                : "The suite read since does not hold it: it was not added, unless the server is still running the call."}
          </p>
          {reread?.status === "failed" && (
            <FailureView failure={reread.failure} compact />
          )}
          <div className="row">
            <button
              type="button"
              disabled={reread?.status === "reading"}
              onClick={() => {
                void store.reread();
                bumpRefresh();
              }}
            >
              {reread?.status === "reading" ? "Reading…" : "Refresh"}
            </button>
            {reread?.status === "read" && dismiss}
          </div>
        </div>
      );
    }
  }
}

// Appends a registered analyzer to the suite (INTERFACES §2): the form
// opens on request, "Add…" asks for confirmation, and the confirmed request
// is sent once. Its outcome stays until dismissed or replaced.
export function AddAnalyzer({ description }: { description: Description }) {
  const store = useAnalyzerAdds();
  const { bumpRefresh } = useData();
  const [confirming, setConfirming] = useState(false);
  const form = store.form();
  const stored = store.outcome();
  // An outcome belongs to the database it was sent to.
  const outcome =
    stored !== null && stored.sent.database === description.database
      ? stored
      : null;
  const name =
    form.name !== null && description.registered.includes(form.name)
      ? form.name
      : defaultName(description);
  const sending = outcome?.kind === "sending";
  const blocker =
    name === null
      ? "this host registers no analyzers"
      : store.sending()
        ? "an add is in flight"
        : store.waiting()
          ? "the last add may have been stored: Refresh first"
          : form.evaluation.kind === "at" && form.evaluation.time.trim() === ""
            ? "enter the evaluation timestamp"
            : null;
  const send = (params: Sent["params"]) => {
    void store.add(
      {
        database: description.database,
        params,
        suite: description.suite,
      },
      bumpRefresh,
    );
  };
  const locked = confirming || store.sending();
  return (
    <section className="add-analyzer" aria-label="Add analyzer">
      {!form.open ? (
        <button type="button" onClick={() => store.setForm({ open: true })}>
          Add an analyzer…
        </button>
      ) : (
        <form
          className="add-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (blocker === null) {
              setConfirming(true);
            }
          }}
        >
          <h3 className="add-title">Add an analyzer</h3>
          <div className="row">
            <label>
              analyzer{" "}
              <select
                value={name ?? ""}
                disabled={locked || name === null}
                onChange={(event) =>
                  store.setForm({ name: event.target.value })
                }
              >
                {description.registered.map((registered) => (
                  <option key={registered} value={registered}>
                    {registered}
                  </option>
                ))}
              </select>
            </label>
            <span className="muted">registered on this host</span>
          </div>
          <div className="row">
            <label>
              configuration{" "}
              <input
                className="configuration"
                size={44}
                spellCheck={false}
                autoComplete="off"
                disabled={locked}
                value={form.configuration}
                onChange={(event) =>
                  store.setForm({ configuration: event.target.value })
                }
              />
            </label>
            <span className="muted">sent verbatim</span>
          </div>
          <EvaluationChoice
            evaluation={form.evaluation}
            disabled={locked}
            onChange={(evaluation) => store.setForm({ evaluation })}
          />
          <div className="row">
            <button type="submit" disabled={blocker !== null || confirming}>
              Add…
            </button>
            <button
              type="button"
              disabled={confirming}
              onClick={() => store.setForm({ open: false })}
            >
              Close
            </button>
            {blocker !== null && (
              <span className="muted" data-testid="add-blocker">
                {blocker}
              </span>
            )}
          </div>
        </form>
      )}
      {confirming && name !== null && (
        <Confirmation
          description={description}
          name={name}
          configuration={form.configuration}
          evaluation={form.evaluation}
          onConfirm={() => {
            setConfirming(false);
            send({
              name,
              configuration: form.configuration,
              ...evaluationParams(form.evaluation),
            });
          }}
          onCancel={() => setConfirming(false)}
        />
      )}
      {sending && (
        <p className="muted" data-testid="add-sending">
          Adding {outcome.sent.params.name}: replaying the whole history…
        </p>
      )}
      {outcome !== null && outcome.kind !== "sending" && (
        <Outcome outcome={outcome} onRetry={(sent) => send(sent.params)} />
      )}
    </section>
  );
}
