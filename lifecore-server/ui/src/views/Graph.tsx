import {
  type MouseEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type {
  DecimalString,
  RenderFormat,
  ViewQueryParams,
} from "../contract.ts";
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

// Where a drawing was last looked at, so it reopens there.
interface Place {
  zoom: number;
  left: number;
  top: number;
}

// One render: the query it was made for, its view pinned by `as_of`, and
// each format as requested.
interface Rendered {
  key: string;
  params: ViewQueryParams;
  view: DecimalString;
  svg: Rendering;
  dot: Rendering;
}

const XLINK = "http://www.w3.org/1999/xlink";
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
    client.call("views.render", { ...target.params, format }).then(
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

const MIN_ZOOM = 0.01;
const MAX_ZOOM = 4;
const STEP = 1.25;
// A new drawing starts fitted, but never so small that its labels cannot be
// read; the frame scrolls instead.
const READABLE = 0.6;
// Below this, a fitted drawing is a shape without legible labels.
const LEGIBLE = 0.05;
// The frame's maximum height as a fraction of the window, as in index.html.
const FRAME_HEIGHT = 0.7;

function clampZoom(zoom: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

interface Size {
  w: number;
  h: number;
}

const SVG_TAG = /<svg\b[^>]*>/;

// A length of the SVG's own size attributes in CSS pixels: Graphviz writes
// points, and a point is 4/3 of a pixel.
function cssLength(value: string | undefined): number | null {
  const match = value === undefined ? null : /^([\d.]+)(pt|px)?$/.exec(value);
  if (match === null || match[1] === undefined) {
    return null;
  }
  const number = Number(match[1]);
  return match[2] === "pt" ? (number * 4) / 3 : number;
}

// The drawing's natural size, read from its markup rather than from layout,
// which a hidden frame does not have: `width` and `height`, else the
// `viewBox`.
export function naturalSize(svg: string): Size | null {
  const tag = SVG_TAG.exec(svg)?.[0] ?? "";
  const attribute = (name: string) =>
    new RegExp(`\\s${name}="([^"]*)"`).exec(tag)?.[1];
  const w = cssLength(attribute("width"));
  const h = cssLength(attribute("height"));
  if (w !== null && h !== null && w > 0 && h > 0) {
    return { w, h };
  }
  const box = attribute("viewBox")
    ?.trim()
    .split(/[\s,]+/)
    .map(Number);
  const [, , bw, bh] = box ?? [];
  return bw !== undefined && bh !== undefined && bw > 0 && bh > 0
    ? { w: bw, h: bh }
    : null;
}

interface Point {
  x: number;
  y: number;
}

// The zoom that shows the whole drawing in the frame's width and the most
// height the frame grows to, FRAME_HEIGHT of the window: the frame grows
// with the drawing, so its own height says nothing yet.
function fit(box: HTMLElement, size: Size): number {
  return clampZoom(
    Math.min(
      1,
      box.clientWidth / size.w,
      (window.innerHeight * FRAME_HEIGHT) / size.h,
    ),
  );
}

// The SVG scrolls inside a frame and zooms with a CSS transform. The sizer
// takes the scaled size, since a transform does not change layout and the
// frame would otherwise scroll over the unscaled box. A new drawing is
// fitted and centered on the first seed it shows (`seeds` holds seed
// positions; the server names node elements `n<position>`) once the frame
// is laid out; a drawing seen before reopens at its zoom and scroll.
// Zooming keeps the point at the middle of the frame in place.
function SvgCanvas({
  svg,
  seeds,
  place,
  onPlace,
}: {
  svg: string;
  seeds: readonly string[];
  place: Place | null;
  onPlace: (place: Place) => void;
}) {
  const open = useOpen();
  const frame = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const natural = useMemo(() => naturalSize(svg), [svg]);
  const [zoom, setZoom] = useState(place?.zoom ?? 1);
  const [tooLarge, setTooLarge] = useState(false);
  // Whether the drawing has been fitted, or reopened where it was; a
  // hidden frame has no width to fit to, so this waits until it has one.
  const placed = useRef(false);
  // Where the drawing was last looked at: the place it reopens at, then
  // every scroll and zoom since.
  const here = useRef(place);
  // What the frame's observer reads: the values of the latest render.
  const latest = useRef({ zoom, seeds, natural, onPlace });
  latest.current = { zoom, seeds, natural, onPlace };
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
  // The seeds are marked once the markup is in the document, and again
  // whenever it is replaced.
  // biome-ignore lint/correctness/useExhaustiveDependencies: see above
  useLayoutEffect(() => {
    const element = inner.current;
    if (element === null) {
      return;
    }
    for (const node of element.querySelectorAll("g.seed")) {
      node.classList.remove("seed");
    }
    for (const position of seeds) {
      element.querySelector(`g#n${position}`)?.classList.add("seed");
    }
  }, [markup, seeds]);
  // The first look at a laid-out frame: a drawing seen before scrolls back
  // to where it was; a new one is fitted, at least READABLE, and centered on
  // its first seed, read on screen and unscaled by the zoom applied.
  useLayoutEffect(() => {
    const box = frame.current;
    const element = inner.current;
    if (box === null || element === null) {
      return;
    }
    const settle = () => {
      const { zoom, seeds, natural } = latest.current;
      const place = here.current;
      if (placed.current || box.clientWidth === 0) {
        return;
      }
      placed.current = true;
      if (place !== null) {
        box.scrollLeft = place.left;
        box.scrollTop = place.top;
        return;
      }
      if (natural === null) {
        return;
      }
      const bounds = element.getBoundingClientRect();
      center.current = null;
      for (const position of seeds) {
        const node = element.querySelector(`g#n${position}`);
        if (node !== null) {
          const rect = node.getBoundingClientRect();
          center.current = {
            x: (rect.left + rect.width / 2 - bounds.left) / zoom,
            y: (rect.top + rect.height / 2 - bounds.top) / zoom,
          };
          break;
        }
      }
      const fitted = fit(box, natural);
      setTooLarge(fitted < LEGIBLE);
      setZoom(Math.max(fitted, READABLE));
      setMoves((count) => count + 1);
    };
    settle();
    // Shown again after being hidden, which loses the scroll: back to where
    // it was, or fitted now if it never was.
    let visible = box.clientWidth > 0;
    const observer = new ResizeObserver(() => {
      const now = box.clientWidth > 0;
      if (now && !visible) {
        const place = here.current;
        if (placed.current && place !== null) {
          box.scrollLeft = place.left;
          box.scrollTop = place.top;
        }
        settle();
      }
      visible = now;
    });
    observer.observe(box);
    return () => observer.disconnect();
  }, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs after each move is laid out
  useLayoutEffect(() => {
    const target = center.current;
    const box = frame.current;
    if (moves === 0 || target === null || box === null) {
      return;
    }
    center.current = null;
    box.scrollLeft = target.x * zoom - box.clientWidth / 2;
    box.scrollTop = target.y * zoom - box.clientHeight / 2;
  }, [moves]);
  // Where the drawing is looked at, kept for its next showing; a hidden
  // frame has nothing worth keeping.
  const remember = () => {
    const box = frame.current;
    if (box !== null && box.clientWidth > 0 && placed.current) {
      here.current = {
        zoom: latest.current.zoom,
        left: box.scrollLeft,
        top: box.scrollTop,
      };
      latest.current.onPlace(here.current);
    }
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: a zoom is part of the place
  useEffect(remember, [zoom, moves]);
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
          onClick={() => {
            const box = frame.current;
            if (natural !== null && box !== null && box.clientWidth > 0) {
              const fitted = fit(box, natural);
              setTooLarge(fitted < LEGIBLE);
              zoomTo(fitted);
            }
          }}
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
          {zoom < 0.1 ? (zoom * 100).toFixed(1) : Math.round(zoom * 100)}%
        </span>
        {tooLarge && (
          <span className="muted">
            too large to fit legibly; use 100% and scroll
          </span>
        )}
        <span className="muted">
          click a node or edge to inspect it; seeds are outlined thick
        </span>
      </div>
      {/* biome-ignore lint/a11y/useKeyWithClickEvents: the SVG's links are the targets; this only routes their clicks */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: as above */}
      <div
        className="graph-frame"
        ref={frame}
        onClick={onClick}
        onScroll={remember}
      >
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
