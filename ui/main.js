// Lemari ("Dokumen") - antarmuka. Enkripsi seluruhnya di Rust.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const AUTO_LOCK_MS = 5 * 60 * 1000;
const THUMB_LIMIT = 25 * 1024 * 1024;
const ICON = { image: "🖼️", video: "🎬", text: "📄", doc: "📑", audio: "🎵", archive: "🗜️", other: "📦" };
const MIME = { jpg: "image/jpeg", jpeg: "image/jpeg", png: "image/png", gif: "image/gif", webp: "image/webp", bmp: "image/bmp",
  heic: "image/heic", heif: "image/heif", tif: "image/tiff", tiff: "image/tiff", svg: "image/svg+xml", mp4: "video/mp4", m4v: "video/mp4", mov: "video/quicktime", webm: "video/webm" };

let items = [], selId = null, kind = "all", busy = false, idle = null;
const urls = new Map(); // id -> blob URL (dibebaskan saat dikunci)

const el = (tag, props = {}, ...kids) => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) { if (k === "class") e.className = v; else if (k.startsWith("on")) e.addEventListener(k.slice(2), v); else e[k] = v; }
  for (const k of kids) e.append(k);
  return e;
};
const call = async (cmd, args) => { try { return await invoke(cmd, args); } catch (e) { throw new Error(typeof e === "string" ? e : (e?.message || String(e))); } };
const show = (v) => { for (const x of ["view-setup", "view-unlock", "view-main"]) $(x).classList.toggle("hidden", x !== v); };
const toast = (m, ms = 2800) => { const t = $("toast"); t.textContent = m; t.classList.remove("hidden"); clearTimeout(toast.h); toast.h = setTimeout(() => t.classList.add("hidden"), ms); };
const ext = (n) => (n.split(".").pop() || "").toLowerCase();
const fmt = (b) => b < 1024 ? b + " B" : b < 1048576 ? (b / 1024).toFixed(0) + " KB" : b < 1073741824 ? (b / 1048576).toFixed(1) + " MB" : (b / 1073741824).toFixed(2) + " GB";

// ---------- awal ----------
async function boot() {
  const s = await call("status");
  if (s.unlocked) return enter();
  show(s.exists ? "view-unlock" : "view-setup");
  (s.exists ? $("unlock-pw") : $("setup-pw")).focus();
}
$("btn-create").onclick = async () => {
  $("setup-err").textContent = "";
  if ($("setup-pw").value !== $("setup-pw2").value) return ($("setup-err").textContent = "Kedua password tidak sama.");
  $("btn-create").disabled = true; $("btn-create").textContent = "Menyiapkan…";
  try { await call("create", { master: $("setup-pw").value }); $("setup-pw").value = $("setup-pw2").value = ""; enter(); }
  catch (e) { $("setup-err").textContent = e.message; }
  finally { $("btn-create").disabled = false; $("btn-create").textContent = "Buat"; }
};
$("setup-pw2").addEventListener("keydown", (e) => e.key === "Enter" && $("btn-create").click());
$("unlock-pw").addEventListener("keydown", async (e) => {
  if (e.key !== "Enter") return;
  $("unlock-err").textContent = ""; $("unlock-pw").disabled = true;
  try { await call("unlock", { master: $("unlock-pw").value }); $("unlock-pw").value = ""; enter(); }
  catch (err) { $("unlock-err").textContent = err.message; }
  finally { $("unlock-pw").disabled = false; $("unlock-pw").focus(); $("unlock-pw").select(); }
});

async function enter() { show("view-main"); selId = null; await refresh(); bump(); }

async function lockNow(msg) {
  closeViewer(); $("menu").classList.add("hidden"); $("modal").classList.add("hidden");
  for (const u of urls.values()) URL.revokeObjectURL(u);
  urls.clear(); items = []; $("grid").replaceChildren();
  await call("lock");
  show("view-unlock"); $("unlock-pw").focus();
  if (msg) toast(msg);
}
$("btn-lock").onclick = () => lockNow();
function bump() { clearTimeout(idle); if (!$("view-main").classList.contains("hidden") && !busy) idle = setTimeout(() => lockNow("Dikunci otomatis"), AUTO_LOCK_MS); }
for (const ev of ["mousemove", "keydown", "click", "wheel"]) document.addEventListener(ev, bump, { passive: true });

// ---------- daftar ----------
async function refresh() {
  items = await call("list", { query: $("search").value.trim(), kind });
  const total = (await call("status")).count;
  const size = items.reduce((a, b) => a + b.size, 0);
  $("count").textContent = `${items.length} dari ${total} file · ${fmt(size)}`;
  $("empty").classList.toggle("hidden", total > 0);
  const g = $("grid"); g.replaceChildren();
  for (const it of items) g.append(tile(it));
  if (!items.find((i) => i.id === selId)) select(null);
}
function tile(it) {
  const thumb = el("div", { class: "thumb" }, ICON[it.kind] || ICON.other);
  const t = el("div", { class: "tile" + (it.id === selId ? " sel" : ""), title: it.name,
    onclick: () => select(it.id), ondblclick: () => openItem(it) },
    thumb, el("div", { class: "tname" }, it.name), el("div", { class: "tsize" }, fmt(it.size)));
  t.dataset.id = it.id;
  if (it.kind === "image" && it.size <= THUMB_LIMIT) lazyThumb(t, thumb, it);
  return t;
}
const io = new IntersectionObserver((ents) => {
  for (const e of ents) if (e.isIntersecting) { io.unobserve(e.target); e.target._load?.(); }
}, { rootMargin: "200px" });
function lazyThumb(t, thumb, it) {
  t._load = async () => {
    try {
      const url = await blobUrl(it);
      const img = el("img", { src: url, onerror: () => img.remove() });
      img.onload = () => { thumb.textContent = ""; thumb.append(img); };
    } catch { /* biarkan ikon */ }
  };
  io.observe(t);
}
async function blobUrl(it) {
  if (urls.has(it.id)) return urls.get(it.id);
  const buf = await call("read_file", { id: it.id });
  const u = URL.createObjectURL(new Blob([buf], { type: MIME[ext(it.name)] || "application/octet-stream" }));
  urls.set(it.id, u);
  return u;
}
function select(id) {
  selId = id;
  for (const t of document.querySelectorAll(".tile")) t.classList.toggle("sel", t.dataset.id === id);
  $("sel-actions").classList.toggle("hidden", !id);
}
$("search").addEventListener("input", () => { clearTimeout(refresh.h); refresh.h = setTimeout(refresh, 120); });
for (const b of $("seg").children) b.onclick = () => { kind = b.dataset.k; for (const x of $("seg").children) x.classList.toggle("on", x === b); refresh(); };
$("grid").addEventListener("click", (e) => { if (e.target === $("grid")) select(null); });

// ---------- menambah ----------
const moveOpt = () => $("opt-move").checked;
function report(r) {
  if (!r) return;
  let m = `${r.added} file diamankan`;
  if (r.moved) m += `, ${r.moved} file asli dihapus`;
  if (r.failed.length) m += `. Gagal: ${r.failed.slice(0, 3).join("; ")}`;
  toast(m, r.failed.length ? 7000 : 3500);
  refresh();
}
async function guard(fn) { if (busy) return; busy = true; clearTimeout(idle); try { await fn(); } catch (e) { toast(e.message, 6000); } finally { busy = false; hideProgress(); bump(); } }
$("btn-add").onclick = () => guard(async () => report(await call("add_dialog", { moveOriginal: moveOpt(), folder: false })));
$("btn-add-folder").onclick = () => guard(async () => report(await call("add_dialog", { moveOriginal: moveOpt(), folder: true })));
$("opt-move").addEventListener("change", (e) => { if (e.target.checked) toast("File asli akan dihapus setelah berhasil diamankan. Pastikan Anda punya cadangan.", 5000); });

listen("tauri://drag-enter", () => { if (!$("view-main").classList.contains("hidden")) $("drop").classList.remove("hidden"); });
listen("tauri://drag-leave", () => $("drop").classList.add("hidden"));
listen("tauri://drag-drop", (e) => {
  $("drop").classList.add("hidden");
  if ($("view-main").classList.contains("hidden")) return;
  const paths = e.payload?.paths || [];
  if (paths.length) guard(async () => report(await call("add_paths", { paths, moveOriginal: moveOpt() })));
});

// ---------- progres ----------
listen("progress", (e) => {
  const p = e.payload;
  $("progress").classList.remove("hidden");
  $("p-label").textContent = p.label;
  $("p-bar").style.width = (p.total ? (p.done / p.total) * 100 : 100) + "%";
  $("p-sub").textContent = `${p.files > 1 ? `File ${p.file} dari ${p.files} · ` : ""}${fmt(p.done)} / ${fmt(p.total)}`;
});
listen("progress-done", hideProgress);
function hideProgress() { $("progress").classList.add("hidden"); $("p-bar").style.width = "0"; }

// ---------- membuka ----------
const current = () => items.find((i) => i.id === selId);
let vIndex = -1;
async function openItem(it) {
  if (!it) return;
  if (it.kind === "image" && it.size <= 64 * 1048576) return viewImage(items.indexOf(it));
  if (it.kind === "text" && it.size <= 4 * 1048576) {
    const buf = await call("read_file", { id: it.id }).catch((e) => toast(e.message));
    if (!buf) return;
    vIndex = -1; $("v-body").replaceChildren(el("pre", {}, new TextDecoder().decode(buf)));
    $("v-name").textContent = it.name; $("v-prev").classList.add("hidden"); $("v-next").classList.add("hidden");
    $("viewer").classList.remove("hidden"); return;
  }
  guard(() => call("open_external", { id: it.id }).then(() => toast("Dibuka dengan aplikasi bawaan. Salinan sementara dihapus saat dikunci.", 4000)));
}
async function viewImage(i) {
  const imgs = items.filter((x) => x.kind === "image");
  const it = items[i]; vIndex = imgs.indexOf(it);
  try {
    const url = await blobUrl(it);
    const img = el("img", { src: url, onerror: () => { closeViewer(); guard(() => call("open_external", { id: it.id })); } });
    $("v-body").replaceChildren(img); $("v-name").textContent = `${it.name} · ${vIndex + 1}/${imgs.length}`;
    $("v-prev").classList.toggle("hidden", imgs.length < 2); $("v-next").classList.toggle("hidden", imgs.length < 2);
    $("viewer").classList.remove("hidden"); select(it.id);
  } catch (e) { toast(e.message); }
}
function step(d) {
  const imgs = items.filter((x) => x.kind === "image");
  if (vIndex < 0 || !imgs.length) return;
  viewImage(items.indexOf(imgs[(vIndex + d + imgs.length) % imgs.length]));
}
function closeViewer() { $("viewer").classList.add("hidden"); $("v-body").replaceChildren(); vIndex = -1; }
$("v-close").onclick = closeViewer; $("v-prev").onclick = () => step(-1); $("v-next").onclick = () => step(1);
document.addEventListener("keydown", (e) => {
  if (!$("viewer").classList.contains("hidden")) {
    if (e.key === "Escape") closeViewer(); else if (e.key === "ArrowLeft") step(-1); else if (e.key === "ArrowRight") step(1);
    return;
  }
  if (e.key === "Escape") { $("menu").classList.add("hidden"); $("modal").classList.add("hidden"); }
  if (document.activeElement.tagName === "INPUT") return;
  if (e.key === "Enter" && selId) openItem(current());
  if ((e.key === "Delete" || e.key === "Backspace") && selId) $("a-delete").click();
});

// ---------- aksi file ----------
$("a-open").onclick = () => openItem(current());
$("a-export").onclick = () => guard(async () => { if (await call("export_one", { id: selId })) toast("File dikeluarkan (tidak terenkripsi)."); });
$("a-rename").onclick = () => {
  const it = current(); if (!it) return;
  ask("Ganti nama", "", [{ id: "rn", value: it.name }], "Simpan", false, async (v) => { await call("rename", { id: it.id, name: v.rn }); refresh(); });
};
$("a-delete").onclick = () => {
  const it = current(); if (!it) return;
  ask("Hapus file?", `"${it.name}" akan dihapus permanen dari brankas. Tindakan ini tidak bisa dibatalkan.`, [], "Hapus", true, async () => {
    await call("delete", { id: it.id });
    if (urls.has(it.id)) { URL.revokeObjectURL(urls.get(it.id)); urls.delete(it.id); }
    selId = null; refresh(); toast("Dihapus");
  });
};

// ---------- menu ----------
$("btn-menu").onclick = (e) => { e.stopPropagation(); $("menu").classList.toggle("hidden"); };
document.addEventListener("click", (e) => { if (!$("menu").contains(e.target)) $("menu").classList.add("hidden"); });
$("menu").addEventListener("click", (e) => {
  const act = e.target.dataset?.act; if (!act) return;
  $("menu").classList.add("hidden");
  if (act === "export-all") guard(async () => { const n = await call("export_all"); if (n != null) toast(`${n} file dikeluarkan (tidak terenkripsi).`, 4000); });
  if (act === "reveal") call("reveal").catch((x) => toast(x.message));
  if (act === "change") ask("Ganti password", "File di dalam tidak perlu dienkripsi ulang.",
    [{ id: "old", label: "Password lama", type: "password" }, { id: "n1", label: "Password baru", type: "password" }, { id: "n2", label: "Ulangi password baru", type: "password" }],
    "Ganti", false, async (v) => { if (v.n1 !== v.n2) throw new Error("Password baru tidak sama."); await call("change_master", { old: v.old, new: v.n1 }); toast("Password diganti"); });
});

// ---------- dialog serbaguna ----------
function ask(title, text, fields, okText, danger, onOk) {
  $("m-title").textContent = title; $("m-text").textContent = text; $("m-err").textContent = "";
  $("m-yes").textContent = okText; $("m-yes").className = danger ? "danger" : "primary";
  const box = $("m-fields"); box.replaceChildren();
  for (const f of fields) {
    const inp = el("input", { id: "mf-" + f.id, type: f.type || "text", value: f.value || "" });
    box.append(f.label ? el("label", {}, f.label, inp) : inp);
  }
  $("modal").classList.remove("hidden");
  const first = box.querySelector("input"); if (first) { first.focus(); first.select(); }
  $("m-yes").onclick = async () => {
    const v = {}; for (const f of fields) v[f.id] = $("mf-" + f.id).value;
    try { $("m-yes").disabled = true; await onOk(v); $("modal").classList.add("hidden"); }
    catch (e) { $("m-err").textContent = e.message; }
    finally { $("m-yes").disabled = false; }
  };
  box.onkeydown = (e) => { if (e.key === "Enter") $("m-yes").click(); };
}
$("m-no").onclick = () => $("modal").classList.add("hidden");

boot();
