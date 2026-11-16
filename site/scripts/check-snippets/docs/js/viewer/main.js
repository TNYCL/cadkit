// main.js: UI only. The worker owns the WebAssembly instance and the parsed document.
const input = document.querySelector("#file");
const view = document.querySelector("#drawing");
const status = document.querySelector("#status");
let worker = startWorker();

function startWorker() {
  const w = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  w.onmessage = ({ data }) => {
    if (data.type === "opened") {
      show(data.svg);
      status.textContent = `${data.info.format} ${data.info.version}: ${data.info.models[0]?.entities ?? 0} entities, ${data.layers.length} layers`;
    } else if (data.type === "picked") {
      status.textContent = data.hit ? `${data.hit.kind} on ${data.hit.layer}` : "nothing here";
    } else if (data.type === "error") {
      status.textContent = `Could not open the file (${data.kind}): ${data.message}`;
      // After a limit error or a trap, start over with a fresh WebAssembly memory.
      if (data.kind === "limit_exceeded" || data.kind === "trap") {
        worker.terminate();
        worker = startWorker();
      }
    }
  };
  return w;
}

function show(svgText) {
  const svg = new DOMParser().parseFromString(svgText, "image/svg+xml").documentElement;
  if (svg.nodeName !== "svg") throw new Error("not an SVG document");
  svg.removeAttribute("width"); // scale with the container; the viewBox keeps the drawing's units
  svg.removeAttribute("height");
  view.replaceChildren(document.importNode(svg, true));
}

input.addEventListener("change", async () => {
  const file = input.files?.[0];
  if (!file) return;
  status.textContent = `Reading ${file.name}...`;
  const bytes = await file.arrayBuffer();
  worker.postMessage({ type: "open", bytes }, [bytes]); // transfer, do not copy
});

view.addEventListener("click", (event) => {
  const svg = view.querySelector("svg");
  const matrix = svg?.getScreenCTM()?.inverse();
  if (!matrix) return;
  // The SVG viewBox is in drawing units with Y pointing down: world Y is the negated SVG Y.
  const p = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix);
  const unitsPerPixel = 1 / (svg.getScreenCTM()?.a || 1);
  worker.postMessage({ type: "pick", x: p.x, y: -p.y, tolerance: 4 * unitsPerPixel });
});
