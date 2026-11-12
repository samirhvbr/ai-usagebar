/**
 * Copying a provider card as an image.
 *
 * The route (proven in both tray engines by the spike behind this feature,
 * .soho/w96/reports/build-9-20261026T140708.md): clone the card, inline the
 * computed styles so the clone is self-contained, embed it in an SVG
 * `foreignObject` (XHTML namespace on the root, the SVG namespace re-declared
 * on nested icons — the data-URL SVG is re-parsed as XML), load the SVG in an
 * `<img>`, draw it scaled onto a `<canvas>`, and `toDataURL`. Self-contained
 * content (system fonts, inline local SVGs) does not taint the canvas in either
 * engine; anything cross-origin would raise a SecurityError here, which the
 * caller reports as a failed copy without ever asking the host.
 */

/** Fixed export scale: a 320 px card becomes a 960 px PNG. */
export const CARD_IMAGE_SCALE = 3;
/** The margin, in CSS pixels, between the card and the image's edge. */
export const CARD_IMAGE_PADDING = 12;
/** No host answer within this window reads as a failed copy. */
export const IMAGE_COPIED_TIMEOUT_MS = 5000;
export const PNG_DATA_URL_PREFIX = "data:image/png;base64,";
/**
 * What an exported card never shows: the header's action buttons (one of them is
 * the copy trigger itself) and the reorder grip. Kept in step with the
 * `[data-exporting]` rules in index.css, which state the same policy.
 */
export const EXPORT_HIDDEN_SELECTOR = ".header-action, .grip";

export interface CardSvgParts {
  /** The card's self-contained HTML (namespacedCardHtml of the clone). */
  html: string;
  /** The card's own height; the image adds the padding on both sides. */
  height: number;
  /** The margin around the card, so its edges never touch the image's. */
  padding: number;
  /** The opaque background's corner radius: the card's own radius. */
  radius: number;
  /** The opaque background: the theme's surface color. */
  surface: string;
  /** The card's own width; the image adds the padding on both sides. */
  width: number;
}

/**
 * The SVG document the card rasterizes in: an opaque rounded background (the
 * export's surface color, the card's radius) with the card's HTML inside a
 * foreignObject, inset by the padding so the card never touches the edge. The
 * XHTML namespace is on the wrapper — without it the XML re-parse drops the
 * HTML semantics and the card renders blank.
 */
export function buildCardSvg(parts: CardSvgParts): string {
  const width = parts.width + parts.padding * 2;
  const height = parts.height + parts.padding * 2;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">` +
    `<rect x="0" y="0" width="${width}" height="${height}" rx="${parts.radius}" fill="${parts.surface}"/>` +
    `<foreignObject x="${parts.padding}" y="${parts.padding}" width="${parts.width}" height="${parts.height}">` +
    `<div xmlns="http://www.w3.org/1999/xhtml" style="width:100%;height:100%">` +
    parts.html +
    `</div></foreignObject></svg>`
  );
}

/**
 * Re-namespace a serialized subtree for the SVG data URL: every `<svg>` icon
 * needs the SVG namespace re-declared or it inherits the XHTML one and vanishes.
 * (The HTML root gets its namespace from buildCardSvg's wrapper.)
 */
export function namespacedCardHtml(html: string): string {
  return html.replace(/<svg\b(?![^>]*xmlns)/g, '<svg xmlns="http://www.w3.org/2000/svg"');
}

/** The base64 body of a `toDataURL("image/png")` result. */
export function stripPngDataUrlPrefix(dataUrl: string): string {
  if (!dataUrl.startsWith(PNG_DATA_URL_PREFIX)) {
    throw new Error(`not a PNG data URL: ${dataUrl.slice(0, 40)}`);
  }
  return dataUrl.slice(PNG_DATA_URL_PREFIX.length);
}

/**
 * The pending copy-image copies, keyed by the id that travels with each command
 * (contract v2): the host answers `window.__AIUB_IMAGE_COPIED__(id, ok)` with the id it
 * RECEIVED, so an answer can only ever address the copy it belongs to. There is no queue
 * and no order dependency: a slow raster that outlives its timeout may dispatch after a
 * newer copy, and a lost answer cannot touch another copy — each id settles exactly once
 * (its own answer, the timeout, or a pre-send failure), and settling removes the id, so
 * a late answer for it is ignored.
 */
export interface CopyRegistry {
  /**
   * Reserve a new pending id (`copy-<n>`, the per-instance counter only grows) and arm
   * its timeout. `onResult(ok)` is reported exactly once: by the host's answer for this
   * id, by the timeout (false), or by `settle` on the pre-send failure path — and the
   * id leaves the pending set either way.
   */
  begin: (onResult: (ok: boolean) => void) => { id: string; settle: (ok: boolean) => void };
  /** Apply the host's answer for one id. Unknown, already-settled and null ids are ignored. */
  answer: (id: string | null, ok: boolean) => void;
  /** A copy is pending: the double-click lock, held from the click until the copy settles. */
  isBusy: () => boolean;
}

interface CopyRegistryOptions {
  /** Pending-copy timeout in ms (default `IMAGE_COPIED_TIMEOUT_MS`). */
  timeoutMs?: number;
  /** Arm an id's timeout (default `globalThis.setTimeout`; tests inject fakes). */
  setTimer?: (task: () => void, ms: number) => unknown;
  /** Disarm it (default `globalThis.clearTimeout`). */
  clearTimer?: (id: unknown) => void;
}

interface CopyRegistryEntry {
  onResult: (ok: boolean) => void;
  timer: unknown;
}

/**
 * Build the pending-copy registry. `globalThis` timers by default so the same logic runs
 * in the browser; tests inject fake timers to drive the expirations directly.
 */
export function createCopyRegistry(options: CopyRegistryOptions = {}): CopyRegistry {
  const timeoutMs = options.timeoutMs ?? IMAGE_COPIED_TIMEOUT_MS;
  const setTimer = options.setTimer ?? ((task: () => void, ms: number) => globalThis.setTimeout(task, ms));
  const clearTimer = options.clearTimer ?? ((id: unknown) => globalThis.clearTimeout(id as number));

  const pending = new Map<string, CopyRegistryEntry>();
  let counter = 0;

  /** Settle one id exactly once and remove it; a missing id is a no-op. */
  const settle = (id: string, ok: boolean): void => {
    const entry = pending.get(id);
    if (!entry) return;
    pending.delete(id);
    clearTimer(entry.timer);
    entry.onResult(ok);
  };

  const begin = (onResult: (ok: boolean) => void): { id: string; settle: (ok: boolean) => void } => {
    counter += 1;
    const id = `copy-${counter}`;
    pending.set(id, { onResult, timer: setTimer(() => settle(id, false), timeoutMs) });
    return { id, settle: (ok: boolean) => settle(id, ok) };
  };

  const answer = (id: string | null, ok: boolean): void => {
    if (typeof id !== "string" || id.length === 0) return; // null or a malformed id
    settle(id, ok);
  };

  const isBusy = (): boolean => pending.size > 0;

  return { answer, begin, isBusy };
}

/**
 * The section to export, WITHOUT interpolating its id into a selector: a macOS account
 * label may carry a double quote, and an id like `anthropic@work"demo` would make
 * `[data-card-id="..."]` an invalid selector that throws before the copy starts.
 * Compare the attribute's value directly instead.
 */
export function findCardElement(root: ParentNode | null, cardId: string): HTMLElement | null {
  if (!root) return null;
  for (const candidate of root.querySelectorAll<HTMLElement>("[data-card-id]")) {
    if (candidate.getAttribute("data-card-id") === cardId) return candidate;
  }
  return null;
}

/**
 * How the clone is serialized into the SVG.
 *
 * The clone must be serialized as XML, not with `outerHTML`: the HTML serializer writes
 * U+00A0 as `&nbsp;` (among other HTML-only entities), and `&nbsp;` does not exist in an
 * XML document — the SVG data URL then fails to load and the copy reports a failure for
 * a card whose only fault is a non-breaking space in its name. The platform's
 * XMLSerializer writes XML escaping (`&amp;`, `&lt;`) and numeric references or literal
 * characters for the rest. Injectable so the pipeline is exercisable where no
 * XMLSerializer exists (bare node).
 */
export interface CardImageOptions {
  serialize?: (clone: Node) => string;
}

const defaultSerialize = (clone: Node): string => new XMLSerializer().serializeToString(clone);

export function serializeCardHtml(clone: Node, options?: CardImageOptions): string {
  return (options?.serialize ?? defaultSerialize)(clone);
}

/**
 * Rasterize one provider card (the `data-card-id` section) to PNG and return the
 * base64 body, without the `data:image/png;base64,` prefix. Never mutates the
 * card on screen: the work happens on a detached clone marked `data-exporting`.
 */
export async function cardImagePng(element: HTMLElement, options?: CardImageOptions): Promise<string> {
  const bounds = element.getBoundingClientRect();
  const width = Math.ceil(bounds.width);
  const height = Math.ceil(bounds.height);
  if (width <= 0 || height <= 0) throw new Error("the card has no size to export");

  const clone = element.cloneNode(true) as HTMLElement;
  // Marks the clone for the [data-exporting] rules in index.css; the clone is
  // detached, so those decisions are also applied inline below.
  clone.setAttribute("data-exporting", "true");
  inlineComputedStyles(element, clone);
  stripExportChrome(element, clone);
  const svg = buildCardSvg({
    html: namespacedCardHtml(serializeCardHtml(clone, options)),
    height,
    padding: CARD_IMAGE_PADDING,
    radius: cssPixelLength("--card-radius"),
    surface: resolvedCssVar("--export-surface"),
    width,
  });
  const dataUrl = await rasterizePng(svg, width + CARD_IMAGE_PADDING * 2, height + CARD_IMAGE_PADDING * 2);
  return stripPngDataUrlPrefix(dataUrl);
}

/** The root element's value for a CSS custom property (theme-scoped by the .dark class). */
function resolvedCssVar(name: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  if (!value) throw new Error(`missing ${name} token`);
  return value;
}

/** A CSS custom property as a pixel length: the export geometry, in the card's own units. */
function cssPixelLength(name: string): number {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const match = /^(-?\d*\.?\d+)px$/.exec(value);
  if (!match) throw new Error(`unresolvable ${name}: ${value}`);
  return Number(match[1]);
}

/**
 * Make the clone self-contained: every element's computed style — resolved (colors
 * computed, custom properties already substituted, box-sizing border-box), inherited
 * values included — moves onto the clone inline. Paired by document order; cloneNode
 * keeps the same shape.
 *
 * Property by property on purpose: Chromium's computed-style `cssText` is empty and
 * WebKit's computed style exposes no enumerable property names, so the per-property
 * `length`/`item()` walk is the one path that works in both tray engines.
 */
function inlineComputedStyles(source: HTMLElement, clone: HTMLElement) {
  const originals = [source, ...source.querySelectorAll("*")];
  const copies = [clone, ...clone.querySelectorAll("*")];
  for (let i = 0; i < originals.length && i < copies.length; i++) {
    const copy = copies[i] as HTMLElement;
    const computed = getComputedStyle(originals[i]);
    for (let property = 0; property < computed.length; property++) {
      const name = computed.item(property);
      const value = computed.getPropertyValue(name);
      if (value !== "") copy.style.setProperty(name, value);
    }
  }
}

/**
 * Screen chrome is not card content: hide the export's own trigger and the other
 * header shortcuts (the [data-exporting] selector), and drop the focused
 * control's ring (outline or ring box-shadow) — focus is a screen state.
 * The card's fill also goes opaque here, where the index.css rule
 * ([data-exporting] .card-surface) cannot reach the detached clone: the theme's
 * surface color, via the --export-card token, even for the translucent
 * Native materials.
 */
function stripExportChrome(source: HTMLElement, clone: HTMLElement) {
  const originals = [source, ...source.querySelectorAll("*")];
  const copies = [clone, ...clone.querySelectorAll("*")];
  const focused = document.activeElement;
  for (let i = 0; i < originals.length && i < copies.length; i++) {
    const original = originals[i];
    const copy = copies[i] as HTMLElement;
    if (original.matches(EXPORT_HIDDEN_SELECTOR)) {
      copy.style.display = "none";
    }
    if (original === focused) {
      copy.style.outline = "none";
      copy.style.boxShadow = "none";
    }
  }
  for (const surface of Array.from(clone.querySelectorAll<HTMLElement>(".card-surface, .lifted-surface"))) {
    surface.style.backgroundColor = resolvedCssVar("--export-card");
  }
}

/** SVG data URL → `<img>` → canvas at the fixed export scale → `toDataURL`. */
async function rasterizePng(svg: string, width: number, height: number): Promise<string> {
  const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  const image = new Image();
  await new Promise<void>((resolve, reject) => {
    image.onload = () => resolve();
    image.onerror = () => reject(new Error("the card SVG failed to load"));
    image.src = url;
  });
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * CARD_IMAGE_SCALE);
  canvas.height = Math.round(height * CARD_IMAGE_SCALE);
  const context = canvas.getContext("2d");
  if (!context) throw new Error("no 2d canvas");
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/png");
}
