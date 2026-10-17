import { useRef, useState } from "react";
import type {
  DecimalString,
  RenderFormat,
  ViewQueryParams,
} from "../contract.ts";
import { useData } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import type { Place } from "../model/drawing.ts";
import { RpcError } from "../rpc.ts";
import { SvgCanvas } from "./SvgCanvas.tsx";

type Rendering =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; text: string }
  | { status: "failed"; failure: unknown };

// One render: the query it was made for, its view pinned by `as_of`, and
// each format as requested.
interface Rendered {
  key: string;
  params: ViewQueryParams;
  view: DecimalString;
  svg: Rendering;
  dot: Rendering;
}

// Drawings kept for Back: each query's last one.
const KEPT = 6;
// Above this many nodes, Render warns that Graphviz may take long.
export const LARGE_DRAWING = 200;

// The shown Explore query drawn by the server's Graphviz, at the view of the
// result on show (`params` carries its boundary as `as_of`). Nothing is
// requested until Render is pressed: a large result can take seconds, and
// the DOT text is fetched only while its toggle is on. The last drawing of
// each query is kept, so Back to a drawn query shows its drawing.
export function GraphView({
  params,
  seeds,
  nodes,
  edges,
}: {
  params: ViewQueryParams | null;
  seeds: readonly string[];
  nodes: number;
  edges: number;
}) {
  const { client } = useData();
  const key = params === null ? null : JSON.stringify(params);
  const [drawings, setDrawings] = useState<Rendered[]>([]);
  const [showDot, setShowDot] = useState(false);
  // Where each kept drawing was last looked at; not state, since nothing
  // renders from it until a drawing is shown again.
  const places = useRef(new Map<string, Place>());
  const rendered =
    drawings.find((drawing) => drawing.key === key) ?? drawings[0] ?? null;
  // An answer lands only on the render it was requested for.
  const request = (target: Rendered, format: RenderFormat) => {
    const settle = (rendering: Rendering) =>
      setDrawings((current) =>
        current.map((drawing) =>
          drawing === target || drawing.key === target.key
            ? { ...drawing, [format]: rendering }
            : drawing,
        ),
      );
    client.call("views.render", { query: target.params, format }).then(
      (result) => settle({ status: "ready", text: result.text }),
      (failure: unknown) => settle({ status: "failed", failure }),
    );
  };
  const render = () => {
    if (params === null || key === null) {
      return;
    }
    const target: Rendered = {
      key,
      params,
      view: params.as_of ?? "",
      svg: { status: "loading" },
      dot: showDot ? { status: "loading" } : { status: "idle" },
    };
    // A new drawing of a query starts afresh; a dropped one forgets its
    // place.
    const kept = [
      target,
      ...drawings.filter((drawing) => drawing.key !== key),
    ].slice(0, KEPT);
    for (const known of [...places.current.keys()]) {
      if (known === key || !kept.some((drawing) => drawing.key === known)) {
        places.current.delete(known);
      }
    }
    setDrawings(kept);
    request(target, "svg");
    if (showDot) {
      request(target, "dot");
    }
  };
  const toggleDot = (on: boolean) => {
    setShowDot(on);
    if (on && rendered !== null && rendered.dot.status === "idle") {
      const target = rendered;
      setDrawings((current) =>
        current.map((drawing) =>
          drawing.key === target.key
            ? { ...drawing, dot: { status: "loading" } }
            : drawing,
        ),
      );
      request(target, "dot");
    }
  };
  const loading =
    rendered !== null &&
    (rendered.svg.status === "loading" || rendered.dot.status === "loading");
  const large = nodes > LARGE_DRAWING;
  return (
    <section data-testid="graph">
      <div className="row">
        <button
          type="button"
          disabled={params === null || loading}
          onClick={render}
        >
          Render
        </button>
        <span
          className={large ? "graph-size large" : "graph-size muted"}
          data-testid="graph-size"
        >
          {nodes} node{nodes === 1 ? "" : "s"}, {edges} edge
          {edges === 1 ? "" : "s"}
          {large && "; large drawing, Graphviz may take long"}
        </span>
        <label>
          <input
            type="checkbox"
            checked={showDot}
            onChange={(event) => toggleDot(event.target.checked)}
          />{" "}
          DOT
        </label>
        {rendered === null && (
          <span className="muted">
            draws the result with Graphviz on the server
          </span>
        )}
        {loading && <span className="muted">rendering…</span>}
        {rendered !== null && (
          <span className="muted" data-testid="graph-view">
            drawn at view [0, {rendered.view})
          </span>
        )}
        {rendered !== null && rendered.key !== key && (
          <span className="stale-note" data-testid="graph-stale">
            drawn for another query or view; Render to update
          </span>
        )}
      </div>
      {rendered !== null && showDot && <DotText rendering={rendered.dot} />}
      {rendered !== null && (
        <SvgResult
          key={rendered.key}
          rendering={rendered.svg}
          seeds={rendered.key === key ? seeds : []}
          place={places.current.get(rendered.key) ?? null}
          onPlace={(place) => places.current.set(rendered.key, place)}
        />
      )}
    </section>
  );
}

function RenderFailure({ failure }: { failure: unknown }) {
  const kind =
    failure instanceof RpcError && "data" in failure.error
      ? failure.error.data?.kind
      : undefined;
  return (
    <>
      <FailureView failure={failure} />
      {kind === "graphviz_unavailable" && (
        <p className="muted">
          SVG needs Graphviz's <code>dot</code> program on the server's machine;
          the DOT toggle shows the text without it.
        </p>
      )}
    </>
  );
}

function DotText({ rendering }: { rendering: Rendering }) {
  switch (rendering.status) {
    case "idle":
      return null;
    case "loading":
      return <p className="muted">loading DOT…</p>;
    case "failed":
      return <RenderFailure failure={rendering.failure} />;
    case "ready":
      return (
        <pre className="raw dot" data-testid="dot">
          {rendering.text}
        </pre>
      );
  }
}

function SvgResult({
  rendering,
  ...canvas
}: {
  rendering: Rendering;
  seeds: readonly string[];
  place: Place | null;
  onPlace: (place: Place) => void;
}) {
  switch (rendering.status) {
    case "idle":
    case "loading":
      return null;
    case "failed":
      return <RenderFailure failure={rendering.failure} />;
    case "ready":
      return <SvgCanvas svg={rendering.text} {...canvas} />;
  }
}
