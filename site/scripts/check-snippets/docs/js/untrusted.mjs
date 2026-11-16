import { readFileSync, writeFileSync } from "node:fs";
import { read } from "cadkit-wasm/node";

const MIB = 1024 * 1024;
let doc;
try {
  doc = read(readFileSync("upload.dxf"), {
    maxInputBytes: 64 * MIB,
    maxDecompressedBytes: 256 * MIB,
    maxObjects: 500_000,
    maxDepth: 32,
  });
  // A complete SVG within 16 MiB, or a limit_exceeded error: never a truncated drawing.
  writeFileSync("upload.svg", doc.toSvgBounded({ widthPx: 1200, maxOutputBytes: 16 * MIB, images: false }));
} catch (error) {
  // error.kind: unknown_format, unsupported, invalid, truncated, limit_exceeded, io, bad_argument
  console.log("rejected:", error.kind, error.message);
} finally {
  doc?.free();
}
