// State
let conversationHistory = [];
let isStreaming = false;
let isRecording = false;
let recognition = null;
let currentArtifact = "genesis-os-manifest.md";

// Initialize Lucide Icons
function refreshIcons() {
  if (window.lucide && typeof window.lucide.createIcons === "function") {
    window.lucide.createIcons();
  }
}

// Markdown Formatter using Marked.js & Highlight.js from Context7
function renderMarkdown(text) {
  if (!text) return "";
  let rawHtml = "";
  if (window.marked && typeof window.marked.parse === "function") {
    try {
      rawHtml = window.marked.parse(text, { gfm: true, breaks: true });
    } catch (e) {
      rawHtml = escapeHtml(text);
    }
  } else {
    rawHtml = escapeHtml(text).replace(/\n/g, "<br>");
  }
  return window.DOMPurify ? window.DOMPurify.sanitize(rawHtml) : escapeHtml(text);
}

function applyHighlighting(element) {
  if (window.hljs) {
    element.querySelectorAll("pre code").forEach((block) => {
      window.hljs.highlightElement(block);
    });
  }
}

function escapeHtml(str) {
  if (!str) return "";
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

// Tab Navigation
document.querySelectorAll(".tab-btn").forEach(btn => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab-btn").forEach(b => b.classList.remove("active"));
    document.querySelectorAll(".tab-pane").forEach(p => p.classList.remove("active"));
    
    btn.classList.add("active");
    const target = btn.getAttribute("data-tab");
    const pane = document.getElementById("pane-" + target);
    if (pane) pane.classList.add("active");

    if (target === "stage") refreshArtifactsList();
    if (target === "goals") loadGoals();
    if (target === "cortex") searchCortexUI();
    if (target === "skills") loadSkills();
    if (target === "adapters") loadAdaptersUI();
    if (target === "pulse") loadPulse();
    if (target === "vault") loadVault();
    if (target === "constitution") loadConstitution();
    if (target === "modelhub") loadModelHub();
    if (target === "mail") loadMailUI();
    refreshIcons();
  });
});

// Companion Dock Collapse / Expand Toggle
function toggleDockCollapse() {
  const dock = document.getElementById("companion-dock");
  if (dock) {
    dock.classList.toggle("collapsed");
    refreshIcons();
  }
}

// Prevent collapse when clicking inside dock actions or buttons
document.querySelector(".dock-actions")?.addEventListener("click", (e) => {
  e.stopPropagation();
});

// Chat Input Key Listener
document.getElementById("chat-input")?.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendMessage();
  }
});

// Artifact Switcher & Canvas Renderer
async function switchArtifact(filename) {
  currentArtifact = filename;
  const titleEl = document.getElementById("current-artifact-title");
  const badgeEl = document.getElementById("artifact-type-badge");
  const frameEl = document.getElementById("artifact-frame");
  const mdViewEl = document.getElementById("artifact-markdown-view");

  if (titleEl) titleEl.textContent = filename;

  const isHtml = filename.endsWith(".html");
  if (badgeEl) {
    badgeEl.textContent = isHtml ? "INTERACTIVE HTML" : "MARKDOWN";
    badgeEl.style.borderColor = isHtml ? "var(--accent-cyan)" : "var(--accent-gold)";
    badgeEl.style.color = isHtml ? "var(--accent-cyan)" : "var(--accent-gold)";
  }

  if (isHtml) {
    mdViewEl.style.display = "none";
    frameEl.style.display = "block";
    frameEl.src = `/artifacts/${encodeURIComponent(filename)}`;
  } else {
    frameEl.style.display = "none";
    mdViewEl.style.display = "block";
    mdViewEl.innerHTML = `<div style="color: var(--text-dim);">Loading ${escapeHtml(filename)}...</div>`;
    try {
      const res = await fetch(`/api/artifacts/${encodeURIComponent(filename)}`);
      const text = await res.text();
      mdViewEl.innerHTML = renderMarkdown(text);
      applyHighlighting(mdViewEl);
    } catch (err) {
      mdViewEl.innerHTML = `<div style="color: #ff007f;">Failed to load artifact: ${escapeHtml(err.message)}</div>`;
    }
  }
  refreshIcons();
}

async function refreshArtifactsList() {
  try {
    const res = await fetch("/api/artifacts");
    const data = await res.json();
    const selector = document.getElementById("artifact-selector");
    if (!selector) return;

    selector.innerHTML = "";
    (data.artifacts || []).forEach(art => {
      const opt = document.createElement("option");
      opt.value = art.id;
      opt.textContent = `${art.title} (${art.ext.toUpperCase()})`;
      if (art.id === currentArtifact) opt.selected = true;
      selector.appendChild(opt);
    });

    if (data.artifacts && data.artifacts.length > 0 && !currentArtifact) {
      switchArtifact(data.artifacts[0].id);
    }
  } catch (err) {
    console.warn("Failed to refresh artifacts list:", err);
  }
}

function popOutArtifact() {
  if (currentArtifact) {
    window.open(`/artifacts/${encodeURIComponent(currentArtifact)}`, "_blank");
  }
}

// Chat Streaming
async function sendMessage() {
  const input = document.getElementById("chat-input");
  const text = input.value.trim();
  if (!text || isStreaming) return;

  // If dock is collapsed, expand it
  const dock = document.getElementById("companion-dock");
  if (dock && dock.classList.contains("collapsed")) {
    dock.classList.remove("collapsed");
  }

  input.value = "";
  appendMessage("user", text);

  conversationHistory.push({ role: "user", content: text });
  const assistantMsgEl = appendMessage("assistant", "Connecting to GB10...");
  isStreaming = true;

  try {
    const res = await fetch("/api/chat/stream", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ messages: conversationHistory })
    });

    if (!res.ok) {
      assistantMsgEl.innerHTML = "<strong>AIEN</strong><br><span style='color:#ff007f;'>Failed to connect to model seat.</span>";
      isStreaming = false;
      return;
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let accumulatedContent = "";
    let accumulatedReasoning = "";

    let renderPending = false;
    const updateStreamingUI = () => {
      renderPending = false;
      let thoughtsHtml = "";
      if (accumulatedReasoning) {
        const isOpen = (accumulatedContent.length > 30) ? "" : "open";
        thoughtsHtml = `
          <details class="reasoning-box" ${isOpen}>
            <summary>🧠 AIEN Reasoning Stream (${accumulatedReasoning.trim().split(/\s+/).length} tokens)</summary>
            <div class="thought-content">${renderMarkdown(accumulatedReasoning)}</div>
          </details>`;
      }

      const bodyHtml = accumulatedContent ? renderMarkdown(accumulatedContent) : "<span style='opacity:0.6; font-style: italic;'>Formulating sovereign response...</span>";
      assistantMsgEl.innerHTML = "<strong>AIEN</strong><br>" + thoughtsHtml + bodyHtml;
      scrollToBottom();
    };

    const scheduleStreamRender = () => {
      if (!renderPending) {
        renderPending = true;
        requestAnimationFrame(updateStreamingUI);
      }
    };

    let buffer = "";
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop();

      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed.startsWith("data: ")) continue;
        const dataStr = trimmed.slice(6).trim();
        if (dataStr === "[DONE]") {
          updateStreamingUI();
          applyHighlighting(assistantMsgEl);
          refreshIcons();
          break;
        }

        try {
          const data = JSON.parse(dataStr);
          if (data.content) accumulatedContent += data.content;
          if (data.reasoning) accumulatedReasoning += data.reasoning;
          scheduleStreamRender();
        } catch (_) {}
      }
    }

    updateStreamingUI();
    applyHighlighting(assistantMsgEl);
    refreshIcons();

    // Check if AIEN mentioned or generated an artifact to auto-switch stage
    const artifactMatch = accumulatedContent.match(/([a-zA-Z0-9_-]+\.(?:html|md))/);
    if (artifactMatch) {
      refreshArtifactsList();
    }

    conversationHistory.push({ role: "assistant", content: accumulatedContent });
  } catch (err) {
    assistantMsgEl.innerHTML = "<strong>AIEN</strong><br><span style='color:#ff007f;'>Stream interrupted: " + err.message + "</span>";
  } finally {
    isStreaming = false;
    applyHighlighting(assistantMsgEl);
    scrollToBottom();
    refreshIcons();
  }
}

function appendMessage(role, text) {
  const chatBox = document.getElementById("chat-box");
  const msg = document.createElement("div");
  msg.className = `chat-msg ${role}`;
  if (role === "user") {
    msg.textContent = text;
  } else {
    msg.innerHTML = "<strong>AIEN</strong><br>" + renderMarkdown(text);
    applyHighlighting(msg);
  }
  chatBox.appendChild(msg);
  scrollToBottom();
  return msg;
}

function scrollToBottom() {
  const chatBox = document.getElementById("chat-box");
  if (chatBox) {
    chatBox.scrollTop = chatBox.scrollHeight;
  }
}

// Action Trigger
async function triggerAction(action) {
  appendMessage("user", `[Action Trigger]: ${action}`);
  const statusEl = appendMessage("assistant", `Executing <strong>${action}</strong> on DGX Spark...`);

  try {
    const res = await fetch("/api/action", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action: action })
    });
    const data = await res.json();
    if (data.status === "ok") {
      statusEl.innerHTML = `<strong>AIEN (${action} completed)</strong><br><pre><code>${data.stdout || "Execution successful."}</code></pre>`;
      applyHighlighting(statusEl);
      loadPulse();
      if (action === "goals") loadGoals();
    } else {
      statusEl.innerHTML = `<strong>AIEN (${action} returned code ${data.exit_code})</strong><br><pre style="color: #ff007f;"><code>${data.stderr || data.stdout || data.detail}</code></pre>`;
      applyHighlighting(statusEl);
    }
  } catch (err) {
    statusEl.innerHTML = `<strong>AIEN</strong><br><span style="color: #ff007f;">Action failed: ${err.message}</span>`;
  }
  scrollToBottom();
  refreshIcons();
}

// Goals
async function loadGoals() {
  const container = document.getElementById("goals-list-container");
  if (!container) return;
  container.innerHTML = "<div style='color: var(--text-dim);'>Loading goals from .goals.json...</div>";

  try {
    const res = await fetch("/api/goals");
    const data = await res.json();
    if (!data.goals || data.goals.length === 0) {
      container.innerHTML = "<div class='card' style='color: var(--text-dim);'>No active project goals. Create one above!</div>";
      return;
    }

    let html = "";
    for (const g of data.goals) {
      const isDone = g.status === "Completed";
      const badgeColor = isDone ? "var(--text-dim)" : "var(--accent-green)";
      
      let msHtml = "";
      if (g.milestones && g.milestones.length > 0) {
        msHtml = "<div style='margin-top: 10px; display: flex; flex-direction: column; gap: 6px;'>";
        for (const m of g.milestones) {
          const icon = m.completed ? "check-circle-2" : "circle";
          const mColor = m.completed ? "var(--accent-green)" : "var(--text-dim)";
          msHtml += `<div style="display: flex; align-items: center; gap: 8px; font-size: 13px; color: ${mColor};">
            <i data-lucide="${icon}" style="width: 14px; height: 14px;"></i>
            <span>${escapeHtml(m.description)}</span>
          </div>`;
        }
        msHtml += "</div>";
      }

      html += `
        <div class="card" style="margin-bottom: 12px; border-left: 3px solid ${badgeColor};">
          <div style="display: flex; justify-content: space-between; align-items: flex-start;">
            <div style="font-weight: 600; font-size: 15px; color: #fff;">${escapeHtml(g.title)}</div>
            <span class="badge" style="color: ${badgeColor}; border-color: ${badgeColor}; font-size: 11px;">${escapeHtml(g.status || "Active")}</span>
          </div>
          ${msHtml}
        </div>
      `;
    }
    container.innerHTML = html;
  } catch (err) {
    container.innerHTML = `<div class='card' style='color:#ff007f;'>Failed to load goals: ${escapeHtml(err.message)}</div>`;
  }
  refreshIcons();
}

async function createNewGoalUI() {
  const titleInput = document.getElementById("new-goal-title");
  const msInput = document.getElementById("new-goal-ms");
  const title = titleInput.value.trim();
  if (!title) return;

  const milestones = msInput.value
    .split(",")
    .map(s => s.trim())
    .filter(s => s.length > 0);

  try {
    const res = await fetch("/api/goals", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title, milestones })
    });
    if (res.ok) {
      titleInput.value = "";
      msInput.value = "";
      loadGoals();
    }
  } catch (err) {
    alert("Failed to create goal: " + err.message);
  }
}

// Cortex Search
async function searchCortexUI() {
  const query = (document.getElementById("cortex-query")?.value || "").trim();
  const container = document.getElementById("cortex-results-container");
  if (!container) return;

  container.innerHTML = "<div style='color: var(--text-dim);'>Querying Cortex (FTS5 + Bi-Encoder vectors)...</div>";

  try {
    const res = await fetch(`/api/cortex?q=${encodeURIComponent(query)}&limit=15`);
    const data = await res.json();
    const results = data.results || [];

    if (results.length === 0) {
      container.innerHTML = "<div class='card' style='color: var(--text-dim);'>No Cortex entities matched your query.</div>";
      return;
    }

    let html = `<div style="display: flex; flex-direction: column; gap: 12px;">`;
    for (const r of results) {
      const score = (r.score !== undefined) ? `Score: ${(r.score * 100).toFixed(1)}%` : "";
      html += `
        <div class="card" style="border-left: 3px solid var(--accent-cyan);">
          <div style="display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 6px;">
            <div style="font-weight: 700; font-size: 14px; color: var(--accent-cyan); font-family: var(--font-mono);">${escapeHtml(r.name)}</div>
            <div style="display: flex; gap: 6px;">
              <span class="badge" style="font-size: 10px;">${escapeHtml(r.type || "entity")}</span>
              ${score ? `<span class="badge" style="font-size: 10px; color: var(--accent-green); border-color: var(--accent-green);">${score}</span>` : ""}
            </div>
          </div>
          <div style="font-size: 12.5px; color: var(--text-main); line-height: 1.5; white-space: pre-wrap; font-family: var(--font-mono);">${escapeHtml(r.content)}</div>
        </div>
      `;
    }
    html += `</div>`;
    container.innerHTML = html;
  } catch (err) {
    container.innerHTML = `<div class='card' style='color:#ff007f;'>Cortex search error: ${escapeHtml(err.message)}</div>`;
  }
  refreshIcons();
}

// Skills
async function loadSkills() {
  const container = document.getElementById("skills-results-container");
  if (!container) return;
  container.innerHTML = "<div style='color: var(--text-dim);'>Scanning ~/skills/ for sovereign runbooks...</div>";

  try {
    const res = await fetch("/api/skills");
    const data = await res.json();
    const skills = data.skills || [];

    if (skills.length === 0) {
      container.innerHTML = "<div class='card' style='color: var(--text-dim);'>No skills installed in ~/skills/.</div>";
      return;
    }

    let html = `<div style="display: flex; flex-direction: column; gap: 12px;">`;
    skills.forEach((s, idx) => {
      const scriptBadge = s.has_scripts 
        ? `<span class="badge" style="font-size: 10px; color: var(--accent-cyan); border-color: var(--accent-cyan);">SCRIPTS</span>`
        : "";
      html += `
        <div class="card" style="border-left: 3px solid var(--accent-green);">
          <div style="display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 6px;">
            <div>
              <div style="font-weight: 700; font-size: 14px; color: var(--accent-green); display: flex; align-items: center; gap: 8px;">
                <i data-lucide="zap"></i> ${escapeHtml(s.name)} ${scriptBadge}
              </div>
              <div style="font-size: 11.5px; color: var(--text-dim); font-family: var(--font-mono); margin-top: 2px;">${escapeHtml(s.path)}</div>
            </div>
            <button class="action-pill" onclick="toggleSkillContent('skill-${idx}')">
              <i data-lucide="file-text"></i> View Runbook
            </button>
          </div>
          <div style="font-size: 12.5px; color: var(--text-main); line-height: 1.5; margin-bottom: 6px;">${escapeHtml(s.description)}</div>
          <div id="skill-${idx}" style="display: none; margin-top: 10px; padding: 12px; background: rgba(0,0,0,0.4); border-radius: 6px; border: 1px solid var(--border-subtle);">
            <pre style="margin: 0; white-space: pre-wrap; font-size: 11.5px; font-family: var(--font-mono); color: var(--text-dim);">${escapeHtml(s.content || "")}</pre>
          </div>
        </div>
      `;
    });
    html += `</div>`;
    container.innerHTML = html;
  } catch (err) {
    container.innerHTML = `<div class='card' style='color:#ff007f;'>Failed to load skills: ${escapeHtml(err.message)}</div>`;
  }
  refreshIcons();
}

function toggleSkillContent(id) {
  const el = document.getElementById(id);
  if (el) el.style.display = (el.style.display === "none" || !el.style.display) ? "block" : "none";
}

// Model Adapters UI
async function loadAdaptersUI() {
  const container = document.getElementById("adapters-grid");
  if (!container) return;
  container.innerHTML = "<div style='color: var(--text-dim);'>Loading adapters from spark-hive...</div>";

  try {
    const res = await fetch("/api/hive/adapters");
    const data = await res.json();
    const adapters = data.adapters || [];

    if (adapters.length === 0) {
      container.innerHTML = "<div class='card' style='color: var(--text-dim);'>No adapters registered.</div>";
      return;
    }

    let html = "";
    for (const a of adapters) {
      html += `
        <div class="card" style="border-top: 2px solid var(--accent-gold);">
          <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
            <div style="font-weight: 700; font-size: 14px; color: #fff;">${escapeHtml(a.title || a.id)}</div>
            <span class="badge" style="font-size: 10px; color: var(--accent-cyan); border-color: var(--accent-cyan);">${escapeHtml(a.engine)}</span>
          </div>
          <p style="font-size: 12px; color: var(--text-muted); line-height: 1.4; margin-bottom: 10px;">${escapeHtml(a.summary)}</p>
          <div style="font-family: var(--font-mono); font-size: 11px; color: var(--accent-green); background: rgba(0,255,136,0.05); padding: 6px 8px; border-radius: 4px; border: 1px solid rgba(0,255,136,0.15);">
            Branch: ${escapeHtml(a.branch_name)}
          </div>
        </div>
      `;
    }
    container.innerHTML = html;
  } catch (err) {
    container.innerHTML = `<div class='card' style='color:#ff007f;'>Failed to load adapters: ${escapeHtml(err.message)}</div>`;
  }
  refreshIcons();
}

// Telemetry Pulse
async function loadPulse() {
  try {
    const res = await fetch("/api/pulse");
    const data = await res.json();

    const bpmEl = document.getElementById("badge-heartbeat-text");
    if (bpmEl && data.heartbeat_bpm) bpmEl.textContent = `${data.heartbeat_bpm} BPM`;

    const cohEl = document.getElementById("badge-coherence-text");
    if (cohEl && data.coherence_score !== undefined) cohEl.textContent = `${(data.coherence_score * 100).toFixed(0)}% Coherence`;

    const modelEl = document.getElementById("val-model");
    if (modelEl && data.subsystems) {
      modelEl.textContent = data.subsystems.model_ok ? "Modular MAX (ONLINE)" : "Modular MAX (OFFLINE)";
      modelEl.style.color = data.subsystems.model_ok ? "var(--accent-green)" : "#ff007f";
    }

    const cortexEl = document.getElementById("val-cortex");
    if (cortexEl && data.subsystems) {
      cortexEl.textContent = data.subsystems.cortex_ok ? "cortex-rs (ONLINE)" : "cortex-rs (OFFLINE)";
      cortexEl.style.color = data.subsystems.cortex_ok ? "var(--accent-green)" : "#ff007f";
    }

    const gpuEl = document.getElementById("val-gpu");
    if (gpuEl && data.hardware && data.hardware.gpu) {
      gpuEl.textContent = `${data.hardware.gpu.name || "GB10"} ${data.hardware.gpu.temperature_c || 36}°C`;
    }
  } catch (_) {}
}

// Vault UI
async function loadVault() {
  const keysContainer = document.getElementById("vault-keys-list");
  const auditContainer = document.getElementById("vault-audit-result");

  try {
    const res = await fetch("/api/vault");
    const data = await res.json();

    if (keysContainer) {
      const keys = data.vault_keys || ["DISCORD_BOT_TOKEN", "GITHUB_TOKEN", "GEMINI_API_KEY", "OPENAI_API_KEY"];
      keysContainer.innerHTML = keys.map(k => `<div><i data-lucide="key" style="width:13px; height:13px; display:inline-block; vertical-align:middle; margin-right:6px;"></i> ${escapeHtml(k)}: [TPM_SEALED]</div>`).join("");
    }

    if (auditContainer) {
      const stray = data.stray_env_files || [];
      if (stray.length === 0) {
        auditContainer.innerHTML = `<span style="color: var(--accent-green); font-weight: 600;"><i data-lucide="check-circle" style="width:14px; height:14px; display:inline-block; vertical-align:middle;"></i> Clean Status: 0 plaintext .env files detected across workspaces. Zero leak invariant maintained.</span>`;
      } else {
        auditContainer.innerHTML = `<span style="color: #ff007f; font-weight: 600;"><i data-lucide="alert-triangle" style="width:14px; height:14px; display:inline-block; vertical-align:middle;"></i> Alert: Plaintext secret files found: ${stray.map(s => escapeHtml(s)).join(", ")}</span>`;
      }
    }
  } catch (err) {
    if (auditContainer) auditContainer.textContent = "Vault audit failed: " + err.message;
  }
  refreshIcons();
}

// Voice Toggle
function toggleVoice() {
  const SpeechRec = window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!SpeechRec) {
    alert("Speech recognition is not supported in this browser.");
    return;
  }
  const btn = document.getElementById("btn-mic");

  if (isRecording) {
    if (recognition) recognition.stop();
    isRecording = false;
    btn.classList.remove("recording");
    return;
  }

  recognition = new SpeechRec();
  recognition.lang = "en-US";
  recognition.continuous = false;
  recognition.interimResults = false;

  recognition.onstart = () => {
    isRecording = true;
    btn.classList.add("recording");
  };

  recognition.onresult = (event) => {
    const transcript = event.results[0][0].transcript;
    document.getElementById("chat-input").value = transcript;
    sendMessage();
  };

  recognition.onerror = () => {
    isRecording = false;
    btn.classList.remove("recording");
  };

  recognition.onend = () => {
    isRecording = false;
    btn.classList.remove("recording");
  };

  recognition.start();
}

// Bootstrapping
window.addEventListener("DOMContentLoaded", () => {
  refreshIcons();
  refreshArtifactsList();
  switchArtifact("genesis-os-manifest.md");
  loadPulse();
  setInterval(loadPulse, 5000);
});

refreshIcons();
refreshArtifactsList();
switchArtifact("genesis-os-manifest.md");
loadPulse();


// Model Swapper Function
async function swapActiveModel(modelId) {
  appendMessage("user", `[Model Swap]: Swapping active model seat to ${modelId}...`);
  try {
    const res = await fetch("/api/models/swap", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ model_id: modelId })
    });
    const data = await res.json();
    appendMessage("assistant", `<strong>Model Seat Swapped</strong>: ${data.message || "Inference engine ready."}`);
  } catch (err) {
    appendMessage("assistant", `<span style="color:#ff007f;">Failed to swap model: ${err.message}</span>`);
  }
}

// Constitution UI
async function loadConstitution() {
  const container = document.getElementById("constitution-markdown-view");
  if (!container) return;
  try {
    const res = await fetch("/CONSTITUTION.md");
    if (!res.ok) throw new Error("HTTP " + res.status);
    const md = await res.text();
    container.innerHTML = renderMarkdown(md);
    if (window.lucide) lucide.createIcons();
    applyHighlighting(container);
  } catch (err) {
    container.innerHTML = '<div style="color:var(--accent-red);">Failed to load Constitution: ' + escapeHtml(err.message) + '</div>';
  }
}


// Model Hub & Operator Profile UI
async function loadModelHub() {
  try {
    const [opRes, engRes] = await Promise.all([
      fetch("/api/operator"),
      fetch("/api/engine/status")
    ]);
    if (opRes.ok) {
      const opData = await opRes.json();
      const op = opData.operator || {};
      const eng = opData.engine || {};
      const nameInput = document.getElementById("operator-name-input");
      const emailInput = document.getElementById("operator-email-input");
      if (nameInput && op.name) nameInput.value = op.name;
      if (emailInput && op.email) emailInput.value = op.email;
      if (eng.model_id) {
        const mEl = document.getElementById("modelhub-model-id");
        if (mEl) mEl.textContent = eng.model_id;
      }
    }
    if (engRes.ok) {
      const eng = await engRes.json();
      const badge = document.getElementById("modelhub-status-badge");
      const latency = document.getElementById("modelhub-latency");
      const device = document.getElementById("modelhub-device");
      const engineName = document.getElementById("modelhub-engine-name");
      if (badge) {
        badge.textContent = eng.status ? eng.status.toUpperCase() : "ONLINE";
        badge.className = eng.status === "online" ? "badge badge-success" : "badge badge-warning";
      }
      if (latency && eng.latency_ms !== undefined) latency.textContent = `${eng.latency_ms} ms`;
      if (device && eng.device) device.textContent = eng.device;
      if (engineName && eng.active_engine) engineName.textContent = eng.active_engine;
    }
    refreshIcons();
  } catch (err) {
    console.error("Failed to load Model Hub data:", err);
  }
}

async function saveOperatorProfile() {
  const name = document.getElementById("operator-name-input")?.value.trim();
  const email = document.getElementById("operator-email-input")?.value.trim();
  const statusEl = document.getElementById("operator-save-status");

  try {
    const curRes = await fetch("/api/operator");
    let payload = curRes.ok ? await curRes.json() : {};
    if (!payload.operator) payload.operator = {};
    payload.operator.name = name;
    payload.operator.email = email;

    const res = await fetch("/api/operator", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });
    if (res.ok) {
      if (statusEl) statusEl.innerHTML = '<span style="color:#00ff66;">✓ Operator profile saved to operator.toml</span>';
    } else {
      if (statusEl) statusEl.innerHTML = '<span style="color:var(--accent-red);">Failed to save profile.</span>';
    }
  } catch (e) {
    if (statusEl) statusEl.innerHTML = `<span style="color:var(--accent-red);">${escapeHtml(e.message)}</span>`;
  }
}

async function installEn2Imprint() {
  const statusEl = document.getElementById("imprint-install-status");
  const btn = document.getElementById("btn-install-imprint");
  if (btn) btn.disabled = true;
  if (statusEl) statusEl.textContent = "Installing EN2 Trinity Imprint into Cortex...";

  try {
    const res = await fetch("/api/imprints/install", { method: "POST" });
    const data = await res.json();
    if (res.ok) {
      if (statusEl) statusEl.innerHTML = `<strong style="color:#00ff66;">✓ Installed ${data.installed_count || 10} foundational lessons into Cortex memory!</strong>`;
    } else {
      if (statusEl) statusEl.innerHTML = `<span style="color:var(--accent-red);">Installation failed: ${escapeHtml(data.error || "Unknown error")}</span>`;
    }
  } catch (e) {
    if (statusEl) statusEl.innerHTML = `<span style="color:var(--accent-red);">${escapeHtml(e.message)}</span>`;
  } finally {
    if (btn) btn.disabled = false;
  }
}

// Sovereign Mail UI
async function loadMailUI() {
  const container = document.getElementById("mail-inbox-list");
  if (!container) return;

  try {
    const [statusRes, inboxRes] = await Promise.all([
      fetch("/api/mail/status"),
      fetch("/api/mail/inbox")
    ]);

    if (statusRes.ok) {
      const st = await statusRes.json();
      const storageEl = document.getElementById("mail-storage-path");
      const smtpEl = document.getElementById("mail-smtp-port");
      const apiEl = document.getElementById("mail-api-port");
      const cortexBadge = document.getElementById("mail-cortex-badge");

      if (storageEl) storageEl.textContent = st.storage_path || "~/.local/share/sovereign/mail";
      if (smtpEl) smtpEl.textContent = `127.0.0.1:${st.smtp_port || 2525}`;
      if (apiEl) apiEl.textContent = `127.0.0.1:${st.api_port || 18092}`;
      if (cortexBadge) {
        cortexBadge.textContent = st.cortex_connected ? "CONNECTED" : "DISCONNECTED";
        cortexBadge.className = st.cortex_connected ? "badge badge-success" : "badge badge-warning";
      }
    }

    if (inboxRes.ok) {
      const msgs = await inboxRes.json();
      if (!msgs || msgs.length === 0) {
        container.innerHTML = '<div style="color:var(--text-muted); font-size:13px; padding:12px;">No incoming messages in local inbox.</div>';
        return;
      }

      container.innerHTML = msgs.map(m => `
        <div class="card" style="background: rgba(0,0,0,0.25); border: 1px solid var(--border-color); padding: 12px; margin-bottom: 6px;">
          <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom: 6px;">
            <strong style="font-size:14px; color:var(--text-main);">${escapeHtml(m.subject || "(No Subject)")}</strong>
            <span style="font-size:11px; color:var(--text-muted);">${escapeHtml(m.received_at ? m.received_at.slice(0, 19).replace("T", " ") : "")}</span>
          </div>
          <div style="display:flex; justify-content:space-between; align-items:center; font-size:12px; color:var(--text-muted); margin-bottom: 8px;">
            <span>From: <code style="color:var(--accent-blue);">${escapeHtml(m.from || "unknown")}</code></span>
            ${m.cortex_indexed ? '<span class="badge badge-success" style="font-size:10px;">CORTEX INDEXED</span>' : ''}
          </div>
          <div style="font-size:13px; line-height:1.5; color:var(--text-main); background: rgba(0,0,0,0.3); padding: 10px; border-radius: 6px; white-space: pre-wrap; font-family: monospace;">${escapeHtml(m.body || "")}</div>
        </div>
      `).join("");
    }
    refreshIcons();
  } catch (err) {
    container.innerHTML = `<div style="color:var(--accent-red); font-size:13px;">Failed to load mailbox: ${escapeHtml(err.message)}</div>`;
  }
}

function showMailComposeModal() {
  const box = document.getElementById("mail-compose-box");
  if (box) box.style.display = "block";
}

function hideMailComposeModal() {
  const box = document.getElementById("mail-compose-box");
  if (box) box.style.display = "none";
}

async function sendSovereignMail() {
  const to = document.getElementById("mail-compose-to")?.value.trim();
  const subject = document.getElementById("mail-compose-subject")?.value.trim();
  const body = document.getElementById("mail-compose-body")?.value.trim();
  const statusEl = document.getElementById("mail-send-status");

  if (!to || !subject || !body) {
    if (statusEl) statusEl.innerHTML = '<span style="color:var(--accent-red);">Please fill in To, Subject, and Body fields.</span>';
    return;
  }

  if (statusEl) statusEl.textContent = "Sending message...";

  try {
    const res = await fetch("/api/mail/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ to: [to], subject, body })
    });

    if (res.ok) {
      if (statusEl) statusEl.innerHTML = '<span style="color:#00ff66;">✓ Sovereign message sent & recorded to Cortex memory!</span>';
      document.getElementById("mail-compose-subject").value = "";
      document.getElementById("mail-compose-body").value = "";
      setTimeout(() => {
        hideMailComposeModal();
        loadMailUI();
      }, 1200);
    } else {
      const err = await res.json();
      if (statusEl) statusEl.innerHTML = `<span style="color:var(--accent-red);">Failed: ${escapeHtml(err.error || "Relay error")}</span>`;
    }
  } catch (e) {
    if (statusEl) statusEl.innerHTML = `<span style="color:var(--accent-red);">${escapeHtml(e.message)}</span>`;
  }
}
