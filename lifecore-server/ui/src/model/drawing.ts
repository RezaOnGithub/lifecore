export const MIN_ZOOM = 0.01;
export const MAX_ZOOM = 4;
export const STEP = 1.25;
// A new drawing starts fitted, but never so small that its labels cannot be
// read; the frame scrolls instead.
export const READABLE = 0.6;
// Below this, a fitted drawing is a shape without legible labels.
export const LEGIBLE = 0.05;
// The frame's maximum height as a fraction of the window, as in index.html.
export const FRAME_HEIGHT = 0.7;

export interface Size {
  w: number;
  h: number;
}

export interface Point {
  x: number;
  y: number;
}

// Where a drawing was last looked at, so it reopens there.
export interface Place {
  zoom: number;
  left: number;
  top: number;
}

export function clampZoom(zoom: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

export function percent(zoom: number): string {
  return `${zoom < 0.1 ? (zoom * 100).toFixed(1) : Math.round(zoom * 100)}%`;
}

// Graphviz prefixes an XML declaration, a doctype, and comments, which
// mean nothing inside an HTML element.
export function inlineMarkup(svg: string): string {
  return svg.slice(Math.max(0, svg.indexOf("<svg")));
}

const SVG_TAG = /<svg\b[^>]*>/;

// A length of the SVG's own size attributes in CSS pixels: Graphviz writes
// points, and a point is 4/3 of a pixel.
export function cssLength(value: string | undefined): number | null {
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

// The zoom that shows the whole drawing in the frame's width and the most
// height the frame grows to, FRAME_HEIGHT of the window: the frame grows
// with the drawing, so its own height says nothing yet.
export function fit(
  size: Size,
  frameWidth: number,
  windowHeight: number,
): number {
  return clampZoom(
    Math.min(1, frameWidth / size.w, (windowHeight * FRAME_HEIGHT) / size.h),
  );
}

// The zoom state of a drawing's canvas. Each action's points are measured
// on screen from the drawing's top left, so at the `zoom` on show, and are
// unscaled by it.
export interface Canvas {
  zoom: number;
  // Whether the last fit was below LEGIBLE.
  tooLarge: boolean;
  // Whether the drawing has been fitted, or reopened where it was; a
  // hidden frame has no width to fit to, so this waits until it has one.
  placed: boolean;
  // The last change of zoom, which the canvas applies to its scroll once
  // laid out: `center` is the unscaled point to bring to the middle of the
  // frame, or null to leave the scroll. Each move is a new object, so a
  // move that keeps the zoom is applied too.
  move: { center: Point | null } | null;
}

export type CanvasAction =
  // The first look at a laid-out frame. A drawing seen before, or one
  // without a size, is not fitted (`fit` is null); a new one is fitted, at
  // least READABLE, and centered on `seed`, the middle of the first seed it
  // shows, if any.
  | { type: "place"; fit: { zoom: number; seed: Point | null } | null }
  // Zooming keeps `middle`, the middle of the frame, in place; fitting also
  // says whether the drawing fits legibly.
  | { type: "fit"; zoom: number; middle: Point }
  | { type: "zoom"; to: number | "in" | "out"; middle: Point };

export function openCanvas(place: Place | null): Canvas {
  return { zoom: place?.zoom ?? 1, tooLarge: false, placed: false, move: null };
}

function unscaled(point: Point, zoom: number): Point {
  return { x: point.x / zoom, y: point.y / zoom };
}

function zoomed(state: Canvas, zoom: number, middle: Point): Canvas {
  return {
    ...state,
    zoom: clampZoom(zoom),
    move: { center: unscaled(middle, state.zoom) },
  };
}

export function canvasReducer(state: Canvas, action: CanvasAction): Canvas {
  switch (action.type) {
    case "place": {
      // A drawing is placed once: a second look before the first one
      // renders measured the same frame.
      if (state.placed) {
        return state;
      }
      if (action.fit === null) {
        return { ...state, placed: true };
      }
      const { zoom, seed } = action.fit;
      return {
        zoom: clampZoom(Math.max(zoom, READABLE)),
        tooLarge: zoom < LEGIBLE,
        placed: true,
        move: { center: seed === null ? null : unscaled(seed, state.zoom) },
      };
    }
    case "fit":
      return {
        ...zoomed(state, action.zoom, action.middle),
        tooLarge: action.zoom < LEGIBLE,
      };
    case "zoom": {
      const to =
        action.to === "in"
          ? state.zoom * STEP
          : action.to === "out"
            ? state.zoom / STEP
            : action.to;
      return zoomed(state, to, action.middle);
    }
  }
}
