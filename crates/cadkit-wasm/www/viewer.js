// cadkit viewer: everything runs locally; files are never uploaded.
// Serve crates/cadkit-wasm/ (e.g. `python -m http.server` there) and open /www/.

const $ = (id) => document.getElementById(id);
const el = {
  app: $("app"), wrap: $("stage-wrap"), stage: $("stage"), drop: $("drop"), busy: $("busy"),
  coords: $("coords"), status: $("status"), file: $("file-input"), open: $("open-btn"),
  theme: $("theme-btn"), fit: $("fit-btn"), zin: $("zoom-in"), zout: $("zoom-out"),
  panelBtn: $("panel-btn"), modelCard: $("model-card"), model: $("model-select"),
  codepage: $("codepage-select"), text: $("text-toggle"), images: $("images-toggle"),
  hiddenLayers: $("hidden-layers-toggle"), selCard: $("sel-card"), selClear: $("sel-clear"),
  selInfo: $("sel-info"), info: $("info"), warnBox: $("warn-box"), warnSum: $("warn-sum"),
  warnList: $("warn-list"), layers: $("layers"), all: $("layers-all"), none: $("layers-none"),
  gmlCrs: $("gml-crs"), gmlMetadata: $("gml-metadata"),
  expSvg: $("exp-svg"), expDxf: $("exp-dxf"), expGml: $("exp-gml"), expJson: $("exp-json"),
};

let wasm = null;       // module namespace once initialised
let doc = null;        // current CadDocument handle
let lastBytes = null;  // raw file bytes for re-reading when fallback codepage changes
let fileName = "drawing";
let infoData = null;
let layerMeta = new Map();
let view = null;       // {cx, cy, s, bx, by, bw, bh}: center, units per pixel, home box
let svgEl = null;
let pickBoxEl = null;
let selectedEntity = null;
let darkSheet = false;

// Full 256-entry AutoCAD Color Index (ACI) palette.
const ACI = [
  "#000000", "#ff0000", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#ff00ff", "#ffffff",
  "#808080", "#c0c0c0", "#ff0000", "#ff7f7f", "#a50000", "#a55252", "#7f0000", "#7f3f3f",
  "#4c0000", "#4c2626", "#260000", "#261313", "#ff3f00", "#ff9f7f", "#a52900", "#a56752",
  "#7f1f00", "#7f4f3f", "#4c1300", "#4c2f26", "#260900", "#261713", "#ff7f00", "#ffbf7f",
  "#a55200", "#a57c52", "#7f3f00", "#7f5f3f", "#4c2600", "#4c3926", "#261300", "#261c13",
  "#ffbf00", "#ffdf7f", "#a57c00", "#a59152", "#7f5f00", "#7f6f3f", "#4c3900", "#4c4226",
  "#261c00", "#262113", "#ffff00", "#ffff7f", "#a5a500", "#a5a552", "#7f7f00", "#7f7f3f",
  "#4c4c00", "#4c4c26", "#262600", "#262613", "#bfff00", "#dfff7f", "#7ca500", "#91a552",
  "#5f7f00", "#6f7f3f", "#394c00", "#424c26", "#1c2600", "#212613", "#7fff00", "#bfff7f",
  "#52a500", "#7ca552", "#3f7f00", "#5f7f3f", "#264c00", "#394c26", "#132600", "#1c2613",
  "#3fff00", "#9fff7f", "#29a500", "#67a552", "#1f7f00", "#4f7f3f", "#134c00", "#2f4c26",
  "#092600", "#172613", "#00ff00", "#7fff7f", "#00a500", "#52a552", "#007f00", "#3f7f3f",
  "#004c00", "#264c26", "#002600", "#132613", "#00ff3f", "#7fff9f", "#00a529", "#52a567",
  "#007f1f", "#3f7f4f", "#004c13", "#264c2f", "#002609", "#135817", "#00ff7f", "#7fffbf",
  "#00a552", "#52a57c", "#007f3f", "#3f7f5f", "#004c26", "#264c39", "#002613", "#13581c",
  "#00ffbf", "#7fffdf", "#00a57c", "#52a591", "#007f5f", "#3f7f6f", "#004c39", "#264c42",
  "#00261c", "#135858", "#00ffff", "#7fffff", "#00a5a5", "#52a5a5", "#007f7f", "#3f7f7f",
  "#004c4c", "#264c4c", "#002626", "#135858", "#00bfff", "#7fdfff", "#007ca5", "#5291a5",
  "#005f7f", "#3f6f7f", "#00394c", "#26427e", "#001c26", "#135858", "#007fff", "#7fbfff",
  "#0052a5", "#527ca5", "#003f7f", "#3f5f7f", "#00264c", "#26397e", "#001326", "#131c58",
  "#003fff", "#7f9fff", "#0029a5", "#5267a5", "#001f7f", "#3f4f7f", "#00134c", "#262f7e",
  "#000926", "#131758", "#0000ff", "#7f7fff", "#0000a5", "#5252a5", "#00007f", "#3f3f7f",
  "#00004c", "#26267e", "#000026", "#131358", "#3f00ff", "#9f7fff", "#2900a5", "#6752a5",
  "#1f007f", "#4f3f7f", "#13004c", "#2f267e", "#090026", "#171358", "#7f00ff", "#bf7fff",
  "#5200a5", "#7c52a5", "#3f007f", "#5f3f7f", "#26004c", "#39267e", "#130026", "#1c1358",
  "#bf00ff", "#df7fff", "#7c00a5", "#9152a5", "#5f007f", "#6f3f7f", "#39004c", "#42264c",
  "#1c0026", "#581358", "#ff00ff", "#ff7fff", "#a500a5", "#a552a5", "#7f007f", "#7f3f7f",
  "#4c004c", "#4c264c", "#260026", "#581358", "#ff00bf", "#ff7fdf", "#a5007c", "#a55291",
  "#7f005f", "#7f3f6f", "#4c0039", "#4c2642", "#26001c", "#581358", "#ff007f", "#ff7fbf",
  "#a50052", "#a5527c", "#7f003f", "#7f3f5f", "#4c0026", "#4c2639", "#260013", "#58131c",
  "#ff003f", "#ff7f9f", "#a50029", "#a55267", "#7f001f", "#3f7f4f", "#4c0013", "#4c262f",
  "#260009", "#581317", "#000000", "#656565", "#666666", "#999999", "#cccccc", "#ffffff",
];

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
  for (const b of [el.fit, el.zin, el.zout, el.all, el.none, el.expSvg, el.expDxf, el.expGml, el.expJson]) b.disabled = !on;
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

function sheetBackground() {
  return darkSheet ? "#12161c" : "#ffffff";
}

function swatch(color) {
  if (color && color.type === "rgb") return `rgb(${color.r},${color.g},${color.b})`;
  if (color && color.type === "aci") {
    if (color.index === 0) return "transparent";
    if (color.index === 7) return darkSheet ? "#ffffff" : "#000000";
    return ACI[color.index] || "#808080";
  }
  return "transparent";
}

function fmtCoord(v) {
  if (!Number.isFinite(v)) return "0";
  const step = view ? Math.max(view.s, 1e-9) : 1e-3;
  const dec = Math.max(0, Math.min(6, Math.ceil(-Math.log10(step)) + 1));
  const s = v.toFixed(dec);
  return dec > 0 ? s.replace(/\.?0+$/, "") || "0" : s;
}

function screenToWorld(clientX, clientY) {
  const r = el.stage.getBoundingClientRect();
  const w = Math.max(1, r.width), h = Math.max(1, r.height);
  const px = clientX - r.left, py = clientY - r.top;
  const svgX = view.cx + (px - w / 2) * view.s;
  const svgY = view.cy + (py - h / 2) * view.s;
  // SVG flips WCS Y (`svg_y = -wcs_y`).
  return { wx: svgX, wy: -svgY };
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

// ---------- selection & hit-testing ----------
function updatePickBox() {
  if (pickBoxEl) { pickBoxEl.remove(); pickBoxEl = null; }
  if (!svgEl || !selectedEntity || !selectedEntity.bbox || !view) return;
  const [minX, minY] = selectedEntity.bbox.min;
  const [maxX, maxY] = selectedEntity.bbox.max;
  const pad = Math.max(3 * view.s, 1e-6);
  const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
  rect.setAttribute("class", "pick-box");
  rect.setAttribute("x", String(minX - pad));
  rect.setAttribute("y", String(-maxY - pad));
  rect.setAttribute("width", String(Math.max(maxX - minX, 0) + 2 * pad));
  rect.setAttribute("height", String(Math.max(maxY - minY, 0) + 2 * pad));
  svgEl.append(rect);
  pickBoxEl = rect;
}

function clearSelection() {
  selectedEntity = null;
  if (pickBoxEl) { pickBoxEl.remove(); pickBoxEl = null; }
  el.selCard.hidden = true;
  el.selInfo.replaceChildren();
}

function showSelection(hit) {
  selectedEntity = hit;
  if (!hit) {
    clearSelection();
    return;
  }
  const rows = [
    ["Kind", hit.kind],
    ["Layer", hit.layer || "0"],
  ];
  if (hit.id !== null && hit.id !== undefined) rows.push(["ID", String(hit.id)]);
  if (hit.color) rows.push(["Color", hit.color]);
  if (hit.block) rows.push(["Block", hit.block]);
  if (hit.text) rows.push(["Text", hit.text]);
  if (hit.bbox) {
    const [x0, y0] = hit.bbox.min;
    const [x1, y1] = hit.bbox.max;
    rows.push(["Bounds", `(${fmtCoord(x0)}, ${fmtCoord(y0)}) – (${fmtCoord(x1)}, ${fmtCoord(y1)})`]);
  }
  if (Array.isArray(hit.attributes) && hit.attributes.length > 0) {
    for (const a of hit.attributes.slice(0, 8)) {
      const val = typeof a.value === "object" ? JSON.stringify(a.value) : String(a.value);
      rows.push([`Attr ${a.tag}`, val]);
    }
  }
  if (hit.props && typeof hit.props === "object") {
    for (const [k, v] of Object.entries(hit.props).slice(0, 6)) {
      rows.push([k, typeof v === "object" ? JSON.stringify(v) : String(v)]);
    }
  }
  el.selInfo.replaceChildren();
  for (const [k, v] of rows) {
    const dt = document.createElement("dt"), dd = document.createElement("dd");
    dt.textContent = k;
    dd.textContent = v;
    el.selInfo.append(dt, dd);
  }
  el.selCard.hidden = false;
  updatePickBox();
}

el.selClear.addEventListener("click", clearSelection);

const pointers = new Map();
let pinch = null;

el.stage.addEventListener("pointerdown", (e) => {
  if (!view) return;
  el.stage.setPointerCapture(e.pointerId);
  pointers.set(e.pointerId, { x: e.clientX, y: e.clientY, sx: e.clientX, sy: e.clientY, moved: false });
  el.stage.classList.add("dragging");
  if (pointers.size === 2) pinch = pinchState();
});
el.stage.addEventListener("pointermove", (e) => {
  if (view) {
    const { wx, wy } = screenToWorld(e.clientX, e.clientY);
    el.coords.hidden = false;
    el.coords.textContent = `X: ${fmtCoord(wx)}, Y: ${fmtCoord(wy)}`;
  }
  const p = pointers.get(e.pointerId);
  if (!p) return;
  if (Math.hypot(e.clientX - p.sx, e.clientY - p.sy) > 4) p.moved = true;
  const r = el.stage.getBoundingClientRect();
  if (pointers.size === 1) {
    panBy(e.clientX - p.x, e.clientY - p.y);
  }
  p.x = e.clientX;
  p.y = e.clientY;
  if (pointers.size === 2 && pinch) {
    p.moved = true;
    const now = pinchState();
    if (pinch.dist > 0 && now.dist > 0) zoomAt(now.dist / pinch.dist, now.mx - r.left, now.my - r.top);
    panBy(now.mx - pinch.mx, now.my - pinch.my);
    pinch = now;
  }
});
el.stage.addEventListener("pointerleave", () => {
  if (pointers.size === 0) el.coords.hidden = true;
});
function endPointer(e) {
  const p = pointers.get(e.pointerId);
  const wasClick = p && !p.moved && pointers.size === 1 && e.type === "pointerup" && e.button === 0;
  pointers.delete(e.pointerId);
  pinch = pointers.size === 2 ? pinchState() : null;
  if (pointers.size === 0) el.stage.classList.remove("dragging");
  if (wasClick && doc && view && typeof doc.pickEntity === "function") {
    try {
      const { wx, wy } = screenToWorld(e.clientX, e.clientY);
      const excludedLayers = [...el.layers.querySelectorAll("input:not(:checked)")].map(cb => cb.dataset.layer);
      const excludedKinds = [];
      if (!el.text.checked) excludedKinds.push("text", "mtext");
      if (!el.images.checked) excludedKinds.push("image");
      const hit = doc.pickEntity(Number(el.model.value) || 0, wx, wy, 8 * view.s, el.hiddenLayers.checked, { excludedLayers, excludedKinds });
      showSelection(hit);
    } catch (err) {
      showStatus(err.message, true, err.kind);
    }
  }
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
  updatePickBox();
}, { passive: false });

el.stage.addEventListener("dblclick", fitView);

el.stage.addEventListener("keydown", (e) => {
  if (!view || e.ctrlKey || e.metaKey || e.altKey) return;
  const step = 60;
  const keys = {
    ArrowLeft: () => panBy(step, 0), ArrowRight: () => panBy(-step, 0),
    ArrowUp: () => panBy(0, step), ArrowDown: () => panBy(0, -step),
    "+": () => zoomAt(1.25), "=": () => zoomAt(1.25), "-": () => zoomAt(0.8), "_": () => zoomAt(0.8),
    "0": fitView, f: fitView, F: fitView, Escape: clearSelection,
  };
  const fn = keys[e.key];
  if (fn) { e.preventDefault(); fn(); }
});

new ResizeObserver(() => applyView()).observe(el.stage);
el.fit.addEventListener("click", fitView);
el.zin.addEventListener("click", () => { zoomAt(1.4); updatePickBox(); });
el.zout.addEventListener("click", () => { zoomAt(1 / 1.4); updatePickBox(); });

el.theme.addEventListener("click", () => {
  darkSheet = !darkSheet;
  el.wrap.classList.toggle("dark-sheet", darkSheet);
  el.theme.textContent = darkSheet ? "Light sheet" : "Dark sheet";
  if (doc && svgEl) preserveLayerTogglesAndRender(true);
});

// ---------- rendering ----------
function renderModel(index, keepView) {
  const svg = doc.toSvg({
    model: index,
    widthPx: 1600,
    strokePx: 1,
    background: sheetBackground(),
    text: el.text.checked,
    images: el.images.checked,
    expandBlocks: true,
    includeHiddenLayers: el.hiddenLayers.checked,
  });
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
  pickBoxEl = null;
  const [bx, by, bw, bh] = vb.length === 4 && vb.every(Number.isFinite) ? vb : [0, 0, 1, 1];
  const prev = keepView ? view : null;
  view = { cx: 0, cy: 0, s: 1, bx, by, bw: Math.max(bw, 1e-9), bh: Math.max(bh, 1e-9) };
  if (prev && prev.bx === bx && prev.by === by) { view.cx = prev.cx; view.cy = prev.cy; view.s = prev.s; applyView(); }
  else fitView();
  buildLayers();
  updatePickBox();
}

function preserveLayerTogglesAndRender(keepView) {
  if (!doc || !svgEl) return;
  const hiddenLayers = [...el.layers.querySelectorAll("input:not(:checked):not(:disabled)")].map((c) => c.dataset.layer);
  try {
    renderModel(Number(el.model.value), keepView);
    for (const cb of el.layers.querySelectorAll("input")) {
      if (hiddenLayers.includes(cb.dataset.layer)) { cb.checked = false; cb.dispatchEvent(new Event("change")); }
    }
  } catch (e) { showStatus(e.message, true, e.kind); }
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
    cb.addEventListener("change", () => {
      if (g) g.style.display = cb.checked ? "" : "none";
      if (!cb.checked && selectedEntity?.layer === name) showSelection(null);
    });
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
    } else if (meta && (meta.frozen || !meta.visible)) {
      const off = document.createElement("span");
      off.className = "off";
      off.textContent = "frozen";
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
    ["Format", String(i.format || "unknown").replace(/_/g, " ").toUpperCase()],
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
async function readBytes(bytes, name, keepView) {
  if (!wasm) { showStatus("The WebAssembly module is not loaded yet.", true); return; }
  showStatus("");
  el.busy.hidden = false;
  await new Promise((r) => setTimeout(r, 20)); // let the busy indicator paint
  try {
    const readOpts = el.codepage.value ? { codepage: el.codepage.value } : undefined;
    const next = wasm.read(bytes, readOpts);
    const prevModel = keepView ? Number(el.model.value) || 0 : 0;
    if (doc) doc.free();
    doc = next;
    lastBytes = bytes;
    fileName = name;
    clearSelection();
    infoData = doc.info();
    layerMeta = new Map(doc.layers().map((l) => [l.name, l]));
    fillInfo();
    fillModels();
    el.wrap.classList.remove("empty");
    el.drop.hidden = true;
    if (infoData.models.length === 0) {
      el.stage.replaceChildren();
      svgEl = null; view = null; el.coords.hidden = true;
      setEnabled(false);
      el.expJson.disabled = false;
      showStatus("The file was read but contains no model to draw.", false);
    } else {
      const targetModel = prevModel < infoData.models.length ? prevModel : 0;
      el.model.value = String(targetModel);
      renderModel(targetModel, keepView);
      setEnabled(true);
      if (infoData.warningCount) showStatus(`Opened with ${infoData.warningCount} warning(s); see Information.`, false);
    }
    document.title = `${name} – cadkit viewer`;
  } catch (e) {
    if (typeof WebAssembly !== "undefined" && e instanceof WebAssembly.RuntimeError) {
      // The module trapped (e.g. out of memory); its state is undefined, so start afresh.
      doc = null;
      wasm = null;
      clearSelection();
      setEnabled(false);
      el.stage.replaceChildren();
      svgEl = null; view = null; el.coords.hidden = true;
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

async function loadFile(file) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  await readBytes(bytes, file.name, false);
}

el.model.addEventListener("change", () => {
  clearSelection();
  try { renderModel(Number(el.model.value), false); } catch (e) { showStatus(e.message, true, e.kind); }
});
el.codepage.addEventListener("change", () => {
  if (lastBytes) readBytes(lastBytes, fileName, true);
});
for (const t of [el.text, el.images, el.hiddenLayers]) t.addEventListener("change", () => {
  preserveLayerTogglesAndRender(true);
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
    const svg = doc.toSvg({
      model: Number(el.model.value),
      widthPx: 1600,
      strokePx: 1,
      background: sheetBackground(),
      text: el.text.checked,
      images: el.images.checked,
      expandBlocks: true,
      includeHiddenLayers: el.hiddenLayers.checked,
    });
    download(`${stem()}.svg`, svg, "image/svg+xml");
  } catch (e) { showStatus(e.message, true, e.kind); }
});
el.expDxf.addEventListener("click", () => {
  try { download(`${stem()}.dxf`, doc.toDxf("r2018"), "application/dxf"); }
  catch (e) { showStatus(e.message, true, e.kind); }
});
el.expGml.addEventListener("click", () => {
  try {
    const crs = el.gmlCrs.value.trim();
    if (!crs) { showStatus("Enter the CRS identifier of the existing coordinates before exporting GML.", true); el.gmlCrs.focus(); return; }
    const res = JSON.parse(doc.toCityGmlWithReport(crs, 1, el.gmlMetadata.checked));
    download(`${stem()}.gml`, res.xml, "application/gml+xml");
    if (res.report && res.report.metadata_only_entities > 0) {
      showStatus(`Exported CityGML (${res.report.metadata_only_entities} non-surface annotation(s) kept as metadata).`, false);
    }
  } catch (e) { showStatus(e.message, true, e.kind); }
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
