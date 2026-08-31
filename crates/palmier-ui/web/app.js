const $ = (id) => document.getElementById(id);

async function api(path, options) {
  const response = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...options,
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error || `HTTP ${response.status}`);
  return body;
}

let toastTimer;
function toast(message, bad = false) {
  const el = $("toast");
  el.textContent = message;
  el.classList.toggle("bad", bad);
  el.classList.add("is-on");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove("is-on"), 2600);
}

// ------------------------------------------------------------------ tabs
for (const tab of document.querySelectorAll(".tab")) {
  tab.addEventListener("click", () => {
    for (const t of document.querySelectorAll(".tab")) t.classList.remove("is-on");
    for (const v of document.querySelectorAll(".view")) v.classList.remove("is-on");
    tab.classList.add("is-on");
    $(`view-${tab.dataset.view}`).classList.add("is-on");
    if (tab.dataset.view !== "setup") refreshProject();
  });
}

// ----------------------------------------------------------------- setup
function dot(el, state) {
  el.className = `dot ${state}`;
}

async function refreshStatus() {
  const s = await api("/api/status");
  $("version").textContent = `v${s.version}`;
  $("connect").textContent = s.connectCommand;
  $("desktop-config").textContent = JSON.stringify(
    { mcpServers: { palmier: { command: "palmier.exe", args: ["serve", "--stdio"] } } },
    null, 2,
  );

  const ready = s.ffmpeg.ready;
  dot($("ffmpeg-stat").querySelector(".dot"), ready ? "ok" : "bad");
  $("ffmpeg-label").textContent = ready ? "FFmpeg ready" : "FFmpeg not found";
  $("ffmpeg-sub").textContent = ready
    ? "Rendering and media import will work."
    : `${s.ffmpeg.missing.join(" and ")} missing from PATH.`;

  const open = s.project.open;
  dot($("project-stat").querySelector(".dot"), open ? "ok" : "warn");
  $("project-label").textContent = open ? (s.project.path ?? "In memory") : "No project open";
  $("project-sub").textContent = open
    ? (s.project.unsaved ? "Unsaved changes." : "Saved.")
    : "Open or create one under Project.";

  $("health").innerHTML =
    `<span class="pill"><span class="dot ok"></span>server on ${new URL(s.mcpUrl).port}</span>` +
    (s.jobsRunning ? `<span class="pill"><span class="dot warn"></span>${s.jobsRunning} running</span>` : "");
}

$("copy-connect").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText($("connect").textContent);
    toast("Command copied");
  } catch {
    toast("Could not reach the clipboard", true);
  }
});

// ------------------------------------------------------------------ keys
async function refreshKeys() {
  const { keys } = await api("/api/keys");
  const list = $("keys");
  list.innerHTML = "";
  if (keys.length === 0) {
    list.innerHTML = `<li class="empty">No keys yet. Generation stays unavailable until you add one.</li>`;
    return;
  }
  keys.forEach((hint, i) => {
    const li = document.createElement("li");
    li.innerHTML =
      `<span class="slot">key ${i + 1}</span>` +
      `<span class="hint-key">${hint}</span>` +
      `<span class="spacer"></span>`;
    const remove = document.createElement("button");
    remove.className = "btn ghost";
    remove.textContent = "Remove";
    remove.addEventListener("click", async () => {
      try {
        await api(`/api/keys/${i + 1}`, { method: "DELETE" });
        toast(`Key ${i + 1} removed`);
        refreshKeys();
      } catch (e) { toast(e.message, true); }
    });
    li.append(remove);
    list.append(li);
  });
}

$("keyform").addEventListener("submit", async (event) => {
  event.preventDefault();
  const field = $("newkey");
  const key = field.value.trim();
  if (!key) return;
  try {
    await api("/api/keys", { method: "POST", body: JSON.stringify({ key }) });
    field.value = "";
    toast("Key added");
    refreshKeys();
    refreshStatus();
  } catch (e) { toast(e.message, true); }
});

// --------------------------------------------------------------- project
$("open-project").addEventListener("click", () => sendProject("open"));
$("create-project").addEventListener("click", () => sendProject("create"));

async function sendProject(action) {
  const path = $("projpath").value.trim();
  try {
    await api("/api/project", {
      method: "POST",
      body: JSON.stringify({ action, path: path || null }),
    });
    toast(action === "open" ? "Project opened" : "Project created");
    refreshProject();
    refreshStatus();
  } catch (e) { toast(e.message, true); }
}

async function refreshProject() {
  const p = await api("/api/project");
  const media = $("media");
  if (!p.open) {
    media.innerHTML = `<li class="empty">No project open.</li>`;
    $("tl").innerHTML = `<p class="empty">No project open.</p>`;
    $("tl-meta").textContent = "";
    return;
  }

  media.innerHTML = p.media.length
    ? ""
    : `<li class="empty">No media yet. Import some, or ask your agent to.</li>`;
  for (const m of p.media) {
    const li = document.createElement("li");
    li.innerHTML =
      `<span>${m.name}</span>` +
      `<span class="dur">${m.durationSeconds.toFixed(1)}s</span>` +
      `<span class="ref">${m.mediaRef.slice(0, 8)}</span>`;
    media.append(li);
  }
  // The timeline speaks in mediaRefs; a person needs the file name.
  const names = new Map(p.media.map((m) => [m.mediaRef, m.name]));
  drawTimeline(p.timeline, names);
}

// -------------------------------------------------------------- timeline
function timecode(frames, fps) {
  const total = Math.floor(frames / fps);
  const f = String(frames % fps).padStart(2, "0");
  const s = String(total % 60).padStart(2, "0");
  const m = String(Math.floor(total / 60) % 60).padStart(2, "0");
  const h = String(Math.floor(total / 3600)).padStart(2, "0");
  return `${h}:${m}:${s}:${f}`;
}

function drawTimeline(tl, names = new Map()) {
  $("tl-name").textContent = tl.name;
  $("tl-meta").textContent =
    `${tl.width}×${tl.height} · ${tl.fps} fps · ${timecode(tl.totalFrames, tl.fps)}`;

  const host = $("tl");
  host.innerHTML = "";
  const span = Math.max(tl.totalFrames, 1);

  // A ruler at whole seconds, thinned out so labels never collide.
  const ruler = document.createElement("div");
  ruler.className = "ruler";
  const seconds = Math.ceil(span / tl.fps);
  const step = Math.max(1, Math.ceil(seconds / 12));
  for (let s = 0; s <= seconds; s += step) {
    const mark = document.createElement("span");
    mark.style.left = `${((s * tl.fps) / span) * 100}%`;
    mark.textContent = `${s}s`;
    ruler.append(mark);
  }
  for (const marker of tl.markers ?? []) {
    const pin = document.createElement("i");
    pin.className = "marker";
    pin.style.left = `${(marker.startFrame / span) * 100}%`;
    if (marker.durationFrames > 0) {
      pin.style.width = `${(marker.durationFrames / span) * 100}%`;
      pin.classList.add("range");
    }
    pin.title = marker.comment ? `${marker.name} — ${marker.comment}` : marker.name;
    pin.dataset.name = marker.name;
    ruler.append(pin);
  }
  host.append(ruler);

  if (!tl.tracks.length) {
    host.insertAdjacentHTML("beforeend", `<p class="empty">This timeline has no tracks.</p>`);
    return;
  }

  // Top track first, matching how they composite on screen.
  for (const track of [...tl.tracks].reverse()) {
    const row = document.createElement("div");
    row.className = "track";
    row.innerHTML = `<div class="tracklabel">${track.type} ${track.trackIndex}</div>`;
    const lane = document.createElement("div");
    lane.className = `lane ${track.type === "audio" ? "audio" : ""}`;
    for (const clip of track.clips) {
      const el = document.createElement("div");
      el.className = "clip";
      el.style.left = `${(clip.startFrame / span) * 100}%`;
      el.style.width = `${(clip.durationFrames / span) * 100}%`;
      const label = names.get(clip.mediaRef) ?? clip.mediaRef;
      el.title = `${label} — frames [${clip.startFrame}, ${clip.endFrame}) · ${clip.mediaRef}`;
      el.textContent = label;
      lane.append(el);
    }
    row.append(lane);
    host.append(row);
  }
}

// ------------------------------------------------------------------ boot
async function boot() {
  try {
    await refreshStatus();
    await refreshKeys();
    await refreshProject();
  } catch (e) {
    toast(e.message, true);
  }
}
boot();
setInterval(() => refreshStatus().catch(() => {}), 4000);
