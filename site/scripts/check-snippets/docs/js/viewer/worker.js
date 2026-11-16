// worker.js: parsing, rendering and picking run here, never on the main thread.
import init, { read } from "cadkit-wasm";

const MIB = 1024 * 1024;
const ready = init(); // fetches and instantiates cadkit_wasm_bg.wasm once
let doc = null;

self.onmessage = async ({ data }) => {
  await ready;
  try {
    if (data.type === "open") {
      doc?.free();
      doc = null;
      doc = read(new Uint8Array(data.bytes), { maxInputBytes: 64 * MIB, maxObjects: 2_000_000 });
      // A complete SVG within the budget, or a limit_exceeded error; raster images left out.
      const svg = doc.toSvgBounded({ widthPx: 1600, maxOutputBytes: 32 * MIB, images: false });
      self.postMessage({ type: "opened", info: doc.info(), layers: doc.layers(), svg });
    } else if (data.type === "pick" && doc) {
      // World XY coordinates and tolerance in drawing units; metadata capped at 16 KiB.
      const hit = doc.pickEntityBounded(0, data.x, data.y, data.tolerance, { maxOutputBytes: 16 * 1024 });
      self.postMessage({ type: "picked", hit });
    }
  } catch (error) {
    // error.kind: unknown_format, unsupported, invalid, truncated, limit_exceeded, ...
    // Without a kind it is a WebAssembly trap: this instance is no longer usable.
    self.postMessage({ type: "error", kind: error.kind ?? "trap", message: String(error.message ?? error) });
  }
};
