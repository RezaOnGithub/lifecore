import {
  type MouseEvent,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { RenderFormat, ViewQueryParams } from "../contract.ts";
import { useData } from "../data.tsx";
import { FailureView } from "../failure.tsx";
import { useOpen } from "../links.tsx";
import { inspectHref } from "../navigation.ts";
import { RpcError } from "../rpc.ts";

type Rendering =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; text: string }
  | { status: "failed"; failure: unknown };

// One render: the query it was made for, keyed with the refresh counter so a
// reload marks it as possibly stale, and each format as requested.
interface Rendered {
  key: string;
  params: ViewQueryParams;
  svg: Rendering;
  dot: Rendering;
}

const XLINK = "http://www.w3.org/1999/xlink";

// The submitted Explore query drawn by the server's Graphviz. Nothing is
// requested until Render is pressed: a large result can take seconds, and
// the DOT text is fetched only while its toggle is on.
export function GraphView({
  params,
  focus,
}: {
  params: ViewQueryParams | null;
  focus: readonly string[];
}) {
  const { client, refresh } = useData();
  const key = params === null ? null : JSON.stringify({ params, refresh });
  const [rendered, setRendered] = useState<Rendered | null>(null);
  const [showDot, setShowDot] = useState(false);
  // An answer lands only on the render it was requested for.
  const request = (
    target: { key: string; params: ViewQueryParams },
    format: RenderFormat,
  ) => {
    const settle = (rendering: Rendering) =>
      setRendered((current) =>
        current?.key === target.key
          ? { ...current, [format]: rendering }
          : current,
      );
    client.call("views.render", { ...target.params, format }).then(
      (result) => settle({ status: "ready", text: result.text }),
      (failure: unknown) => settle({ status: "failed", failure }),
    );
  };
  const render = () => {
    if (params === null || key === null) {
      return;
    }
    const target = { key, params };
    setRendered({
      ...target,
      svg: { status: "loading" },
      dot: showDot ? { status: "loading" } : { status: "idle" },
    });
    request(target, "svg");
    if (showDot) {
      request(target, "dot");
    }
  };
  const toggleDot = (on: boolean) => {
    setShowDot(on);
    if (on && rendered !== null && rendered.dot.status === "idle") {
      setRendered({ ...rendered, dot: { status: "loading" } });
      request(rendered, "dot");
    }
  };
  const loading =
    rendered !== null &&
    (rendered.svg.status === "loading" || rendered.dot.status === "loading");
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
        {rendered !== null && rendered.key !== key && (
          <span className="muted" data-testid="graph-stale">
            drawn for an earlier query or reload; Render to update
          </span>
        )}
      </div>
      {rendered !== null && showDot && <DotText rendering={rendered.dot} />}
      {rendered !== null && (
        <SvgResult rendering={rendered.svg} focus={focus} />
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
  focus,
}: {
  rendering: Rendering;
  focus: readonly string[];
}) {
  switch (rendering.status) {
    case "idle":
    case "loading":
      return null;
    case "failed":
      return <RenderFailure failure={rendering.failure} />;
    case "ready":
      return <SvgCanvas svg={rendering.text} focus={focus} />;
  }
}

const MIN_ZOOM = 0.05;
const MAX_ZOOM = 4;
const STEP = 1.25;
// A new drawing starts fitted, but never so small that its labels cannot be
// read; the frame scrolls instead.
const READABLE = 0.6;
// The frame's maximum height as a fraction of the window, as in index.html.
const FRAME_HEIGHT = 0.7;

function clampZoom(zoom: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

interface Point {
  x: number;
  y: number;
}

// The SVG scrolls inside a frame and zooms with a CSS transform. The sizer
// takes the scaled size, since a transform does not change layout and the
// frame would otherwise scroll over the unscaled box. A new drawing is
// centered on the first seed it shows (`focus` holds seed positions; the
// server names node elements `n<position>`), and zooming keeps the point at
// the middle of the frame in place.
function SvgCanvas({ svg, focus }: { svg: string; focus: readonly string[] }) {
  const open = useOpen();
  const frame = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [zoom, setZoom] = useState(1);
  // The unscaled point to bring to the middle of the frame once the zoom
  // has been applied, and a counter that applies it even when the zoom
  // stays the same.
  const center = useRef<Point | null>(null);
  const [moves, setMoves] = useState(0);
  // Graphviz prefixes an XML declaration, a doctype, and comments, which
  // mean nothing inside an HTML element.
  const markup = useMemo(
    () => ({ __html: svg.slice(Math.max(0, svg.indexOf("<svg"))) }),
    [svg],
  );
  // The frame grows with the drawing up to FRAME_HEIGHT of the window, so
  // fitting measures against that height rather than the frame's own.
  const fit = (size: { w: number; h: number }) => {
    const box = frame.current;
    if (box === null || size.w === 0 || size.h === 0) {
      return 1;
    }
    return clampZoom(
      Math.min(
        1,
        box.clientWidth / size.w,
        (window.innerHeight * FRAME_HEIGHT) / size.h,
      ),
    );
  };
  const zoomTo = (next: number) => {
    const box = frame.current;
    if (box !== null) {
      center.current = {
        x: (box.scrollLeft + box.clientWidth / 2) / zoom,
        y: (box.scrollTop + box.clientHeight / 2) / zoom,
      };
      setMoves((count) => count + 1);
    }
    setZoom(clampZoom(next));
  };
  // A new drawing is measured as laid out, which a transform does not
  // change; the seed's place is read on screen and unscaled by the
  // transform still applied from the previous drawing.
  // biome-ignore lint/correctness/useExhaustiveDependencies: measured once per drawing
  useLayoutEffect(() => {
    const element = inner.current;
    if (element === null) {
      return;
    }
    const size = { w: element.offsetWidth, h: element.offsetHeight };
    const box = element.getBoundingClientRect();
    const applied = size.w === 0 ? 1 : box.width / size.w;
    center.current = null;
    for (const position of focus) {
      const node = element.querySelector(`#n${position}`);
      if (node !== null) {
        const rect = node.getBoundingClientRect();
        center.current = {
          x: (rect.left + rect.width / 2 - box.left) / applied,
          y: (rect.top + rect.height / 2 - box.top) / applied,
        };
        break;
      }
    }
    setNatural(size);
    setZoom(Math.max(fit(size), READABLE));
    setMoves((count) => count + 1);
  }, [markup]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs after each move is laid out
  useLayoutEffect(() => {
    const target = center.current;
    const box = frame.current;
    // On mount this effect runs right after the measuring one, before its
    // zoom and size are laid out; the move it asks for comes next.
    if (moves === 0 || target === null || box === null) {
      return;
    }
    center.current = null;
    box.scrollLeft = target.x * zoom - box.clientWidth / 2;
    box.scrollTop = target.y * zoom - box.clientHeight / 2;
  }, [moves]);
  const onClick = (event: MouseEvent<HTMLDivElement>) => {
    const anchor = (event.target as Element).closest("a");
    if (anchor === null) {
      return;
    }
    const id = inspectHref(
      anchor.getAttributeNS(XLINK, "href") ?? anchor.getAttribute("href"),
    );
    if (id !== null) {
      event.preventDefault();
      open({ kind: "inspector", id });
    }
  };
  return (
    <>
      <div className="row zoom">
        <button
          type="button"
          disabled={natural === null}
          onClick={() => natural && zoomTo(fit(natural))}
        >
          fit
        </button>
        <button type="button" onClick={() => zoomTo(1)}>
          100%
        </button>
        <button
          type="button"
          aria-label="zoom out"
          onClick={() => zoomTo(zoom / STEP)}
        >
          −
        </button>
        <button
          type="button"
          aria-label="zoom in"
          onClick={() => zoomTo(zoom * STEP)}
        >
          +
        </button>
        <span className="muted" data-testid="zoom">
          {Math.round(zoom * 100)}%
        </span>
        <span className="muted">click a node or edge to inspect it</span>
      </div>
      {/* biome-ignore lint/a11y/useKeyWithClickEvents: the SVG's links are the targets; this only routes their clicks */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: as above */}
      <div className="graph-frame" ref={frame} onClick={onClick}>
        <div
          className="graph-sizer"
          style={
            natural === null
              ? undefined
              : { width: natural.w * zoom, height: natural.h * zoom }
          }
        >
          <div
            className="graph-svg"
            ref={inner}
            style={{ transform: `scale(${zoom})` }}
            data-testid="graph-svg"
            // Our own server's Graphviz output; labels are XML-escaped.
            // biome-ignore lint/security/noDangerouslySetInnerHtml: see above
            dangerouslySetInnerHTML={markup}
          />
        </div>
      </div>
    </>
  );
}
