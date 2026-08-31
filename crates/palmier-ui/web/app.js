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

  // The panel exists only where Claude Code does; without it, the terminal or the
  // Claude app is the way in.
  $("chat").classList.toggle("is-off", !s.chat?.available);

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

// --------------------------------------------------------------- preview
let currentTimeline = null;
let playhead = 0;
let pending = null;

function clampFrame(frame) {
  const last = Math.max((currentTimeline?.totalFrames ?? 1) - 1, 0);
  return Math.min(Math.max(Math.round(frame), 0), last);
}

/// Fetch one composited frame. Requests are coalesced: a render takes a moment, and
/// clicking three times should show the third frame, not queue three renders.
async function showFrame(frame) {
  if (!currentTimeline) return;
  playhead = clampFrame(frame);
  $("pv-frame").value = playhead;
  drawPlayhead();

  const fps = currentTimeline.fps;
  $("pv-meta").textContent = `frame ${playhead} · ${timecode(playhead, fps)}`;

  if (pending) { pending.wanted = playhead; return; }
  pending = { wanted: playhead };
  $("pv-busy").classList.add("is-on");

  try {
    while (pending) {
      const wanted = pending.wanted;
      const response = await fetch(`/api/frame/${wanted}`, { cache: "no-store" });
      if (!response.ok) {
        const body = await response.json().catch(() => ({}));
        throw new Error(body.error || `HTTP ${response.status}`);
      }
      const blob = await response.blob();
      // Another click landed while this was rendering; go again for the newest.
      if (pending.wanted !== wanted) continue;
      const img = $("pv-img");
      const previous = img.src;
      img.src = URL.createObjectURL(blob);
      img.classList.add("is-on");
      $("pv-empty").classList.add("is-off");
      if (previous.startsWith("blob:")) URL.revokeObjectURL(previous);
      pending = null;
    }
  } catch (e) {
    pending = null;
    toast(e.message, true);
  } finally {
    $("pv-busy").classList.remove("is-on");
  }
}

function drawPlayhead() {
  const span = Math.max(currentTimeline?.totalFrames ?? 1, 1);
  for (const head of document.querySelectorAll(".playhead")) {
    head.style.left = `${(playhead / span) * 100}%`;
  }
}

for (const [id, step] of [["pv-back", -1], ["pv-fwd", 1], ["pv-back10", -10], ["pv-fwd10", 10]]) {
  $(id).addEventListener("click", () => showFrame(playhead + step));
}
$("pv-start").addEventListener("click", () => showFrame(0));
$("pv-end").addEventListener("click", () => showFrame(Infinity));
$("pv-frame").addEventListener("change", (e) => showFrame(Number(e.target.value) || 0));

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
  currentTimeline = tl;
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
    // Click anywhere on a lane to put the playhead there.
    lane.addEventListener("click", (event) => {
      const box = lane.getBoundingClientRect();
      showFrame(((event.clientX - box.left) / box.width) * span);
    });
    const head = document.createElement("div");
    head.className = "playhead";
    lane.append(head);
    row.append(lane);
    host.append(row);
  }
  drawPlayhead();
}

// ------------------------------------------------------------------ chat
let chatSession = null;
let chatBusy = false;

function bubble(kind, text) {
  $("chat-empty")?.remove();
  const el = document.createElement("div");
  el.className = `msg ${kind}`;
  el.textContent = text;
  $("chat-log").append(el);
  $("chat-log").scrollTop = $("chat-log").scrollHeight;
  return el;
}

/// Send a message and stream the reply.
///
/// The panel is a view onto Claude Code, not a second agent: the CLI runs with this
/// server's own MCP endpoint, so it edits the very project on screen.
async function askClaude(prompt) {
  if (chatBusy) return;
  chatBusy = true;
  $("chat-send").disabled = true;
  bubble("you", prompt);
  const status = bubble("thinking", "thinking…");

  try {
    const response = await fetch("/api/chat", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ prompt, sessionId: chatSession }),
    });
    if (!response.ok) {
      const body = await response.json().catch(() => ({}));
      throw new Error(body.error || `HTTP ${response.status}`);
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let touched = false;

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const frames = buffer.split("\n\n");
      buffer = frames.pop() ?? "";

      for (const frame of frames) {
        const line = frame.split("\n").find((l) => l.startsWith("data: "));
        if (!line) continue;
        let event;
        try { event = JSON.parse(line.slice(6)); } catch { continue; }

        if (event.kind === "start") {
          chatSession = event.sessionId ?? chatSession;
          $("chat-meta").textContent = event.model ?? "";
        } else if (event.kind === "say") {
          if (event.text?.trim()) bubble("claude", event.text.trim());
          if (event.tools?.length) {
            touched = true;
            const list = document.createElement("ul");
            list.className = "tools";
            for (const tool of event.tools) {
              const li = document.createElement("li");
              li.textContent = String(tool.name).replace(/^mcp__palmier__/, "");
              list.append(li);
            }
            $("chat-log").append(list);
          }
        } else if (event.kind === "done") {
          status.remove();
          if (event.isError && event.text) bubble("fail", event.text);
          const cost = event.costUsd ? ` · $${event.costUsd.toFixed(4)}` : "";
          const turns = event.turns ? `${event.turns} turns` : "";
          if (turns || cost) bubble("turn", `${turns}${cost}`);
        } else if (event.kind === "error") {
          bubble("fail", event.text);
        }
      }
    }
    status.remove();

    // Whatever it touched, show it: that is the point of the panel being here.
    if (touched) {
      await refreshProject();
      if (currentTimeline) showFrame(playhead);
    }
    refreshStatus().catch(() => {});
  } catch (e) {
    status.remove();
    bubble("fail", e.message);
  } finally {
    chatBusy = false;
    $("chat-send").disabled = false;
  }
}

$("chatform").addEventListener("submit", (event) => {
  event.preventDefault();
  const field = $("chat-input");
  const prompt = field.value.trim();
  if (!prompt) return;
  field.value = "";
  askClaude(prompt);
});

// Enter sends, shift+enter makes a new line.
$("chat-input").addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    $("chatform").requestSubmit();
  }
});

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
