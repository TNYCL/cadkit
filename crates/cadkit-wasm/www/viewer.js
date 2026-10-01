// cadkit viewer: everything runs locally; files are never uploaded.
// Serve crates/cadkit-wasm/ (e.g. `python -m http.server` there) and open /www/.

const $ = (id) => document.getElementById(id);
const el = {
  app: $("app"), wrap: $("stage-wrap"), stage: $("stage"), drop: $("drop"), busy: $("busy"),
  status: $("status"), file: $("file-input"), open: $("open-btn"), fit: $("fit-btn"),
  zin: $("zoom-in"), zout: $("zoom-out"), panelBtn: $("panel-btn"),
  modelCard: $("model-card"), model: $("model-select"), text: $("text-toggle"), images: $("images-toggle"),
  info: $("info"), warnBox: $("warn-box"), warnSum: $("warn-sum"), warnList: $("warn-list"),
  layers: $("layers"), all: $("layers-all"), none: $("layers-none"),
  expSvg: $("exp-svg"), expDxf: $("exp-dxf"), expJson: $("exp-json"),
};

let wasm = null;       // module namespace once initialised
let doc = null;        // current CadDocument handle
let fileName = "drawing";
let infoData = null;
let layerMeta = new Map();
let view = null;       // {cx, cy, s, bx, by, bw, bh}: center, units per pixel, home box
let svgEl = null;

// ---------- helpers ----------
function showStatus(msg, isError, kind) {
  el.status.hidden = !msg;
  el.status.className = "status" + (isError ? " error" : "");
  el.status.replaceChildren();
  if (!msg) return;
  if (kind) {
    const s = document.createElement("strong");
    s.textContent = kind.replace(/_/g, " ") + ": ";
    el.status.append(s);
  }
  el.status.append(document.createTextNode(msg));
}

function setEnabled(on) {
  for (const b of [el.fit, el.zin, el.zout, el.all, el.none, el.expSvg, el.expDxf, el.expJson]) b.disabled = !on;
}

function download(name, data, mime) {
  const url = URL.createObjectURL(new Blob([data], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

const ACI = ["", "#ff0000", "#ffff00", "#00c000", "#00c0c0", "#0000ff", "#c000c0", "#808080", "#808080", "#808080"];
function swatch(color) {
  if (color && color.type === "rgb") return `rgb(${color.r},${color.g},${color.b})`;
  if (color && color.type === "aci") return ACI[color.index] || "#808080";
  return "transparent";
}

// ---------- viewport (pan / zoom through the SVG viewBox) ----------
function stageSize() {
  const r = el.stage.getBoundingClientRect();
  return { w: Math.max(1, r.width), h: Math.max(1, r.height) };
}

function applyView() {
  if (!svgEl || !view) return;
  const { w, h } = stageSize();
  const vw = w * view.s, vh = h * view.s;
  svgEl.setAttribute("viewBox", `${view.cx - vw / 2} ${view.cy - vh / 2} ${vw} ${vh}`);
}

function fitView() {
  if (!view) return;
  const { w, h } = stageSize();
  view.s = Math.max(view.bw / w, view.bh / h) * 1.02;
  view.cx = view.bx + view.bw / 2;
  view.cy = view.by + view.bh / 2;
  applyView();
}

function zoomAt(factor, px, py) {
  if (!view) return;
  const { w, h } = stageSize();
  const x = px === undefined ? w / 2 : px;
  const y = py === undefined ? h / 2 : py;
  const fit = Math.max(view.bw / w, view.bh / h);
  const ns = Math.min(Math.max(view.s / factor, fit * 1e-4), fit * 50);
  // Keep the drawing point under (x, y) fixed.
  const wx = view.cx + (x - w / 2) * view.s, wy = view.cy + (y - h / 2) * view.s;
  view.s = ns;
  view.cx = wx - (x - w / 2) * ns;
  view.cy = wy - (y - h / 2) * ns;
  applyView();
}

function panBy(dx, dy) {
  if (!view) return;
  view.cx -= dx * view.s;
  view.cy -= dy * view.s;
  applyView();
}

const pointers = new Map();
let pinch = null;

el.stage.addEventListener("pointerdown", (e) => {
  if (!view) return;
  el.stage.setPointerCapture(e.pointerId);
  pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
  el.stage.classList.add("dragging");
  if (pointers.size === 2) pinch = pinchState();
});
el.stage.addEventListener("pointermove", (e) => {
  const p = pointers.get(e.pointerId);
  if (!p) return;
  const r = el.stage.getBoundingClientRect();
  if (pointers.size === 1) {
    panBy(e.clientX - p.x, e.clientY - p.y);
  }
  p.x = e.clientX;
  p.y = e.clientY;
  if (pointers.size === 2 && pinch) {
    const now = pinchState();
    if (pinch.dist > 0 && now.dist > 0) zoomAt(now.dist / pinch.dist, now.mx - r.left, now.my - r.top);
    panBy(now.mx - pinch.mx, now.my - pinch.my);
    pinch = now;
  }
});
function endPointer(e) {
  pointers.delete(e.pointerId);
  pinch = pointers.size === 2 ? pinchState() : null;
  if (pointers.size === 0) el.stage.classList.remove("dragging");
}
el.stage.addEventListener("pointerup", endPointer);
el.stage.addEventListener("pointercancel", endPointer);

function pinchState() {
  const [a, b] = [...pointers.values()];
  return { dist: Math.hypot(a.x - b.x, a.y - b.y), mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2 };
}

el.stage.addEventListener("wheel", (e) => {
  if (!view) return;
  e.preventDefault();
  const r = el.stage.getBoundingClientRect();
  let dy = e.deltaY;
  if (e.deltaMode === 1) dy *= 16;
  else if (e.deltaMode === 2) dy *= 100;
  zoomAt(Math.exp(-dy * 0.0015), e.clientX - r.left, e.clientY - r.top);
}, { passive: false });

el.stage.addEventListener("dblclick", fitView);

el.stage.addEventListener("keydown", (e) => {
  if (!view || e.ctrlKey || e.metaKey || e.altKey) return;
  const step = 60;
  const keys = {
    ArrowLeft: () => panBy(step, 0), ArrowRight: () => panBy(-step, 0),
    ArrowUp: () => panBy(0, step), ArrowDown: () => panBy(0, -step),
    "+": () => zoomAt(1.25), "=": () => zoomAt(1.25), "-": () => zoomAt(0.8), "_": () => zoomAt(0.8),
    "0": fitView, f: fitView, F: fitView,
  };
  const fn = keys[e.key];
  if (fn) { e.preventDefault(); fn(); }
});

new ResizeObserver(() => applyView()).observe(el.stage);
el.fit.addEventListener("click", fitView);
el.zin.addEventListener("click", () => zoomAt(1.4));
el.zout.addEventListener("click", () => zoomAt(1 / 1.4));

// ---------- rendering ----------
function renderModel(index, keepView) {
  const svg = doc.toSvg({ model: index, widthPx: 1600, strokePx: 1, background: "#ffffff", text: el.text.checked, images: el.images.checked, expandBlocks: true });
  const parsed = new DOMParser().parseFromString(svg, "image/svg+xml");
  if (parsed.querySelector("parsererror") || parsed.documentElement.localName !== "svg") {
    throw Object.assign(new Error("The renderer produced invalid SVG."), { kind: "invalid" });
  }
  const node = document.importNode(parsed.documentElement, true);
  const vb = (node.getAttribute("viewBox") || "0 0 1 1").trim().split(/[\s,]+/).map(Number);
  node.removeAttribute("width");
  node.removeAttribute("height");
  node.setAttribute("preserveAspectRatio", "none");
  node.setAttribute("focusable", "false");
  node.setAttribute("aria-hidden", "true");
  el.stage.replaceChildren(node);
  svgEl = node;
  const [bx, by, bw, bh] = vb.length === 4 && vb.every(Number.isFinite) ? vb : [0, 0, 1, 1];
  const prev = keepView ? view : null;
  view = { cx: 0, cy: 0, s: 1, bx, by, bw: Math.max(bw, 1e-9), bh: Math.max(bh, 1e-9) };
  if (prev && prev.bx === bx && prev.by === by) { view.cx = prev.cx; view.cy = prev.cy; view.s = prev.s; applyView(); }
  else fitView();
  buildLayers();
}

function buildLayers() {
  el.layers.replaceChildren();
  const groups = new Map();
  for (const g of svgEl.querySelectorAll("g[data-layer]")) groups.set(g.getAttribute("data-layer"), g);
  const names = [...layerMeta.keys()];
  for (const n of groups.keys()) if (!layerMeta.has(n)) names.push(n);
  if (!names.length) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = "No layers";
    el.layers.append(li);
    return;
  }
  for (const name of names) {
    const meta = layerMeta.get(name);
    const g = groups.get(name);
    const li = document.createElement("li");
    const label = document.createElement("label");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = !!g;
    cb.disabled = !g;
    cb.addEventListener("change", () => { if (g) g.style.display = cb.checked ? "" : "none"; });
    cb.dataset.layer = name;
    const sw = document.createElement("span");
    sw.className = "sw";
    sw.style.background = swatch(meta && meta.color);
    const nm = document.createElement("span");
    nm.className = "nm";
    nm.textContent = name || "(unnamed)";
    nm.title = name;
    label.append(cb, sw, nm);
    if (!g) {
      const off = document.createElement("span");
      off.className = "off";
      off.textContent = meta && (meta.frozen || !meta.visible) ? "hidden" : "empty";
      label.append(off);
    }
    li.append(label);
    el.layers.append(li);
  }
}

function setAllLayers(on) {
  for (const cb of el.layers.querySelectorAll("input[type=checkbox]:not(:disabled)")) {
    if (cb.checked !== on) { cb.checked = on; cb.dispatchEvent(new Event("change")); }
  }
}
el.all.addEventListener("click", () => setAllLayers(true));
el.none.addEventListener("click", () => setAllLayers(false));

function fillInfo() {
  const i = infoData;
  const rows = [
    ["File", fileName],
    ["Format", String(i.format || "unknown").replace("_", " ").toUpperCase()],
    ["Version", i.version || "unknown"],
    ["Application", i.application || "unknown"],
    ["Units", String(i.units || "unknown").replace(/_/g, " ")],
    ["Models", String(i.models.length)],
    ["Entities", i.models.reduce((a, m) => a + m.entities, 0).toLocaleString()],
    ["Layers", String(i.layerCount)],
    ["Blocks", String(i.blockCount)],
    ["Warnings", String(i.warningCount)],
  ];
  el.info.replaceChildren();
  for (const [k, v] of rows) {
    const dt = document.createElement("dt"), dd = document.createElement("dd");
    dt.textContent = k;
    dd.textContent = v;
    el.info.append(dt, dd);
  }
  const warns = doc.warnings();
  el.warnBox.hidden = warns.length === 0;
  el.warnSum.textContent = `${warns.length} warning${warns.length === 1 ? "" : "s"}`;
  el.warnList.replaceChildren();
  for (const w of warns.slice(0, 200)) {
    const li = document.createElement("li");
    li.textContent = `${w.code}: ${w.message}`;
    el.warnList.append(li);
  }
  if (warns.length > 200) {
    const li = document.createElement("li");
    li.textContent = `… and ${warns.length - 200} more`;
    el.warnList.append(li);
  }
}

function fillModels() {
  el.model.replaceChildren();
  infoData.models.forEach((m, idx) => {
    const o = document.createElement("option");
    o.value = String(idx);
    o.textContent = `${m.name || "Model " + (idx + 1)} (${m.entities.toLocaleString()})`;
    el.model.append(o);
  });
  el.modelCard.hidden = infoData.models.length === 0;
}

// ---------- loading ----------
async function loadFile(file) {
  if (!wasm) { showStatus("The WebAssembly module is not loaded yet.", true); return; }
  showStatus("");
  el.busy.hidden = false;
  await new Promise((r) => setTimeout(r, 20)); // let the busy indicator paint
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const next = wasm.read(bytes);
    if (doc) doc.free();
    doc = next;
    fileName = file.name;
    infoData = doc.info();
    layerMeta = new Map(doc.layers().map((l) => [l.name, l]));
    fillInfo();
    fillModels();
    el.wrap.classList.remove("empty");
    el.drop.hidden = true;
    if (infoData.models.length === 0) {
      el.stage.replaceChildren();
      svgEl = null; view = null;
      setEnabled(false);
      el.expJson.disabled = false;
      showStatus("The file was read but contains no model to draw.", false);
    } else {
      renderModel(0, false);
      setEnabled(true);
      if (infoData.warningCount) showStatus(`Opened with ${infoData.warningCount} warning(s); see Information.`, false);
    }
    document.title = `${file.name} – cadkit viewer`;
  } catch (e) {
    if (typeof WebAssembly !== "undefined" && e instanceof WebAssembly.RuntimeError) {
      // The module trapped (e.g. out of memory); its state is undefined, so start afresh.
      doc = null;
      wasm = null;
      setEnabled(false);
      el.stage.replaceChildren();
      svgEl = null; view = null;
      el.wrap.classList.add("empty");
      el.drop.hidden = false;
      showStatus("The file was too large or complex for the browser and the reader stopped (" + e.message + "). The engine is being reloaded; you can open another file.", true, "trap");
      await loadWasm();
    } else {
      showStatus(e && e.message ? e.message : String(e), true, e && e.kind);
    }
  } finally {
    el.busy.hidden = true;
  }
}

el.model.addEventListener("change", () => {
  try { renderModel(Number(el.model.value), false); } catch (e) { showStatus(e.message, true, e.kind); }
});
for (const t of [el.text, el.images]) t.addEventListener("change", () => {
  if (!doc || !svgEl) return;
  const hiddenLayers = [...el.layers.querySelectorAll("input:not(:checked):not(:disabled)")].map((c) => c.dataset.layer);
  try {
    renderModel(Number(el.model.value), true);
    for (const cb of el.layers.querySelectorAll("input")) {
      if (hiddenLayers.includes(cb.dataset.layer)) { cb.checked = false; cb.dispatchEvent(new Event("change")); }
    }
  } catch (e) { showStatus(e.message, true, e.kind); }
});

// ---------- file input, drag and drop ----------
el.open.addEventListener("click", () => el.file.click());
el.drop.addEventListener("click", () => el.file.click());
el.drop.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") { e.preventDefault(); el.file.click(); }
});
el.file.addEventListener("change", () => {
  const f = el.file.files && el.file.files[0];
  if (f) loadFile(f);
  el.file.value = "";
});
let dragDepth = 0;
for (const t of [el.wrap, el.drop]) {
  t.addEventListener("dragenter", (e) => { e.preventDefault(); dragDepth++; el.wrap.classList.add("over"); el.drop.classList.add("over"); });
  t.addEventListener("dragover", (e) => { e.preventDefault(); });
  t.addEventListener("dragleave", () => {
    dragDepth = Math.max(0, dragDepth - 1);
    if (!dragDepth) { el.wrap.classList.remove("over"); el.drop.classList.remove("over"); }
  });
  t.addEventListener("drop", (e) => {
    e.preventDefault();
    dragDepth = 0;
    el.wrap.classList.remove("over");
    el.drop.classList.remove("over");
    const f = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
    if (f) loadFile(f);
  });
}
// Dropping outside the canvas must not navigate the page to the file.
window.addEventListener("dragover", (e) => e.preventDefault());
window.addEventListener("drop", (e) => e.preventDefault());

// ---------- export ----------
const stem = () => fileName.replace(/\.[^.]+$/, "") || "drawing";
el.expSvg.addEventListener("click", () => {
  try {
    const svg = doc.toSvg({ model: Number(el.model.value), widthPx: 1600, strokePx: 1, background: "#ffffff", text: el.text.checked, images: el.images.checked, expandBlocks: true });
    download(`${stem()}.svg`, svg, "image/svg+xml");
  } catch (e) { showStatus(e.message, true, e.kind); }
});
el.expDxf.addEventListener("click", () => {
  try { download(`${stem()}.dxf`, doc.toDxf("r2018"), "application/dxf"); }
  catch (e) { showStatus(e.message, true, e.kind); }
});
el.expJson.addEventListener("click", () => {
  try { download(`${stem()}.json`, doc.toJsonString(true), "application/json"); }
  catch (e) { showStatus(e.message, true, e.kind); }
});

el.panelBtn.addEventListener("click", () => {
  const open = el.app.classList.toggle("panels-open");
  el.panelBtn.setAttribute("aria-expanded", String(open));
});

// ---------- start ----------
el.wrap.classList.add("empty");
let loadCount = 0;
async function loadWasm() {
  try {
    // A fresh query string yields a new module instance (and fresh linear memory).
    const mod = await import(`../pkg/cadkit_wasm.js?v=${loadCount++}`);
    await mod.default();
    wasm = mod;
  } catch (e) {
    wasm = null;
    showStatus("Could not load the WebAssembly module. Build it with `python crates/cadkit-wasm/build.py` and serve the crate folder over HTTP (not file://). " + (e && e.message ? e.message : ""), true);
  }
}
loadWasm();
