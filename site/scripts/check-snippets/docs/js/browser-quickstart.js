import init, { read } from "cadkit-wasm";

await init(); // loads cadkit_wasm_bg.wasm next to the module (bundlers resolve the URL)

const input = document.querySelector('input[type="file"]');
input.addEventListener("change", async () => {
  const doc = read(new Uint8Array(await input.files[0].arrayBuffer()));
  try {
    const svg = doc.toSvg({ widthPx: 1200 });
    // As an image: no scripts, no external requests, whatever the drawing contains.
    const img = document.createElement("img");
    img.alt = "Drawing preview";
    img.src = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
    document.body.append(img);
  } finally {
    doc.free();
  }
});
