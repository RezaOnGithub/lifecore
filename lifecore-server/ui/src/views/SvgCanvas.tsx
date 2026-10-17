import {
  type MouseEvent,
  useEffectEvent,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
} from "react";
import { useOpen } from "../links.tsx";
import {
  canvasReducer,
  fit,
  inlineMarkup,
  naturalSize,
  openCanvas,
  type Place,
  type Point,
  percent,
} from "../model/drawing.ts";
import { inspectHref } from "../navigation.ts";

const XLINK = "http://www.w3.org/1999/xlink";

// The middle of the frame, on screen from the drawing's top left.
function middleOf(box: HTMLElement): Point {
  return {
    x: box.scrollLeft + box.clientWidth / 2,
    y: box.scrollTop + box.clientHeight / 2,
  };
}

// The middle of the first seed the drawing shows, on screen from its top
// left.
function firstSeed(
  element: HTMLElement,
  seeds: readonly string[],
): Point | null {
  const bounds = element.getBoundingClientRect();
  for (const position of seeds) {
    const node = element.querySelector(`g#n${position}`);
    if (node !== null) {
      const rect = node.getBoundingClientRect();
      return {
        x: rect.left + rect.width / 2 - bounds.left,
        y: rect.top + rect.height / 2 - bounds.top,
      };
    }
  }
  return null;
}

// The SVG scrolls inside a frame and zooms with a CSS transform. The sizer
// takes the scaled size, since a transform does not change layout and the
// frame would otherwise scroll over the unscaled box. A new drawing is
// fitted and centered on the first seed it shows (`seeds` holds seed
// positions; the server names node elements `n<position>`) once the frame
// is laid out; a drawing seen before reopens at its zoom and scroll.
// Zooming keeps the point at the middle of the frame in place.
export function SvgCanvas({
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
  const [{ zoom, tooLarge, placed, move }, dispatch] = useReducer(
    canvasReducer,
    place,
    openCanvas,
  );
  // Where the drawing was last looked at: the place it reopens at, then
  // every scroll and zoom since. Not state, since every scroll changes it
  // and nothing renders from it.
  const here = useRef(place);
  const markup = useMemo(() => ({ __html: inlineMarkup(svg) }), [svg]);
  // The seeds are marked once the markup is in the document, and again
  // whenever it is replaced, which drops the marks: the effect does not
  // read `markup`, but must follow it.
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
  // A look at the laid-out frame, once mounted and whenever it is shown
  // again after being hidden, which loses the scroll: a drawing seen before
  // scrolls back to where it was. The first look places the drawing, and
  // fits a new one.
  const look = useEffectEvent(() => {
    const box = frame.current;
    const element = inner.current;
    if (box === null || element === null || box.clientWidth === 0) {
      return;
    }
    const place = here.current;
    if (place !== null) {
      box.scrollLeft = place.left;
      box.scrollTop = place.top;
    }
    if (!placed) {
      dispatch({
        type: "place",
        fit:
          place !== null || natural === null
            ? null
            : {
                zoom: fit(natural, box.clientWidth, window.innerHeight),
                seed: firstSeed(element, seeds),
              },
      });
    }
  });
  useLayoutEffect(() => {
    const box = frame.current;
    if (box === null) {
      return;
    }
    look();
    let visible = box.clientWidth > 0;
    const observer = new ResizeObserver(() => {
      const now = box.clientWidth > 0;
      if (now && !visible) {
        look();
      }
      visible = now;
    });
    observer.observe(box);
    return () => observer.disconnect();
  }, []);
  // Where the drawing is looked at, kept for its next showing; a hidden
  // frame has nothing worth keeping.
  const remember = () => {
    const box = frame.current;
    if (box !== null && box.clientWidth > 0 && placed) {
      here.current = { zoom, left: box.scrollLeft, top: box.scrollTop };
      onPlace(here.current);
    }
  };
  // A zoom is part of the place, so a move is remembered as a scroll is.
  const rememberMove = useEffectEvent(remember);
  useLayoutEffect(() => {
    const box = frame.current;
    if (move === null || box === null) {
      return;
    }
    if (move.center !== null) {
      box.scrollLeft = move.center.x * zoom - box.clientWidth / 2;
      box.scrollTop = move.center.y * zoom - box.clientHeight / 2;
    }
    rememberMove();
  }, [move, zoom]);
  const zoomTo = (to: number | "in" | "out") => {
    const box = frame.current;
    if (box !== null) {
      dispatch({ type: "zoom", to, middle: middleOf(box) });
    }
  };
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
              dispatch({
                type: "fit",
                zoom: fit(natural, box.clientWidth, window.innerHeight),
                middle: middleOf(box),
              });
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
          onClick={() => zoomTo("out")}
        >
          −
        </button>
        <button type="button" aria-label="zoom in" onClick={() => zoomTo("in")}>
          +
        </button>
        <span className="muted" data-testid="zoom">
          {percent(zoom)}
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
