"use strict";

(function () {
  const presenceCanvas = document.getElementById("presence-canvas");
  const presenceStage = document.getElementById("presence-stage");
  const presenceLabel = document.getElementById("presence-state-label");
  const orb = window.MagnusPresence ? window.MagnusPresence.create(presenceCanvas) : { setState() {} };

  const promptWrap = document.getElementById("prompt-spotlight-wrap");
  const promptForm = document.getElementById("hero-prompt-form");
  const promptInput = document.getElementById("hero-prompt-input");
  const micBtn = document.getElementById("mic-btn");

  const workspaceStage = document.getElementById("workspace-stage");
  const mainBox = document.getElementById("bento-main");
  const tileA = document.getElementById("bento-tile-a");
  const tileB = document.getElementById("bento-tile-b");
  const tileATitle = document.getElementById("tile-a-title");
  const tileADesc = document.getElementById("tile-a-desc");
  const tileBTitle = document.getElementById("tile-b-title");
  const tileBDesc = document.getElementById("tile-b-desc");

  const reasoningPill = document.getElementById("reasoning-pill");
  const reasoningToggle = document.getElementById("reasoning-toggle");
  const reasoningSummary = document.getElementById("reasoning-summary");
  const reasoningStepsEl = document.getElementById("reasoning-steps");
  const streamOutput = document.getElementById("stream-output");

  const artifactBtn = document.getElementById("open-artifact-btn");
  const artifactCloseBtn = document.getElementById("artifact-close-btn");
  const artifactTitle = document.getElementById("artifact-title");
  const artifactContent = document.getElementById("artifact-content");
  const artifactFootNote = document.getElementById("artifact-foot-note");
  const artifactActionBtn = document.getElementById("artifact-action-btn");
  const artifactMeta = document.getElementById("artifact-trigger-meta");

  let runToken = 0;
  let listening = false;
  let activeWorkflow = null;

  const WORKFLOWS = {
    inbox: {
      prompt: "Clear my morning inbox and draft replies for urgent clients",
      steps: [
        "Scanning 47 overnight threads in Gmail...",
        "Identifying 2 urgent client renewals...",
        "Checking Q4 pricing & calendar availability...",
        "Staging 6 personal drafts in your tone..."
      ],
      doneSummary: "Completed 4 steps · 1.8s",
      tileA: { title: "Gmail & Calendar", desc: "47 threads sorted · 2 renewals flagged" },
      tileB: { title: "Draft Safety", desc: "6 drafts staged · 0 sent without approval" },
      response: "Your morning inbox is clear. I sorted 47 overnight threads, filed newsletters away, and staged 6 personal replies — including two urgent renewal follow-ups with your Q4 pricing attached. Nothing sends until you click approve.",
      artifact: {
        title: "Staged Morning Drafts (6)",
        meta: "Interactive artifact · ready for review",
        foot: "Nothing leaves your inbox until you approve.",
        cta: "Approve & Send Drafts",
        doneCta: "✓ Sent & verified",
        html: `
          <div class="art-card"><div><strong>Northstar Logistics — Annual Renewal</strong><span>Updated 15% volume tier PDF attached · waiting for your OK</span></div><span class="art-tag">Draft Ready</span></div>
          <div class="art-card"><div><strong>Helios Design — Kickoff Times</strong><span>Proposed Tue 10 AM &amp; Thu 2 PM from your live calendar</span></div><span class="art-tag">Draft Ready</span></div>
          <div class="art-card"><div><strong>41 Newsletters &amp; Receipts</strong><span>Filed cleanly into Finance &amp; Reading</span></div><span class="art-tag ok">Organized</span></div>
        `
      }
    },
    deck: {
      prompt: "Pull Q4 sales numbers and finish Friday's board deck",
      steps: [
        "Connecting to live Shopify & revenue ledger...",
        "Refreshing 4 Q4 growth & retention charts...",
        "Updating slides 1–12 in Google Drive...",
        "Drafting executive summary for #leadership..."
      ],
      doneSummary: "Completed 4 steps · 2.1s",
      tileA: { title: "Live Revenue Pull", desc: "$418,200 MRR (+14.2%) verified" },
      tileB: { title: "Google Slides Deck", desc: "12 slides updated · charts refreshed" },
      response: "Friday's board deck is ready. I pulled live Q4 revenue and retention figures, refreshed all four charts across slides 1 through 12, and prepared a three-line executive brief ready to drop into #leadership.",
      artifact: {
        title: "Q4 Board Deck — Live Preview",
        meta: "12 slides updated · speaker notes ready",
        foot: "Every number verified against your live ledger.",
        cta: "Share to #leadership",
        doneCta: "✓ Shared & confirmed",
        html: `
          <div class="art-card"><div><strong>Slide 3 · Monthly Recurring Revenue</strong><span>$418,200 (+14.2% MoM) · North America outpaced forecast</span></div><span class="art-tag ok">Verified</span></div>
          <div class="art-card"><div><strong>Slide 6 · Enterprise Retention</strong><span>118% net dollar retention · 2 renewals closing this week</span></div><span class="art-tag ok">Verified</span></div>
          <div class="art-card"><div><strong>Speaker Notes (Slides 1–12)</strong><span>Concise talking points tailored for a 20-minute review</span></div><span class="art-tag">Ready</span></div>
        `
      }
    },
    proposal: {
      prompt: "Prepare the Northstar renewal proposal so I can add a personal note",
      steps: [
        "Pulling Northstar account usage & renewal tier...",
        "Generating updated annual pricing PDF...",
        "Opening live proposal screen beside chat...",
        "Pausing cleanly for your personal note..."
      ],
      doneSummary: "Completed 4 steps · paused for you",
      tileA: { title: "Magnus Live Screen", desc: "Proposal open · ready for your keyboard" },
      tileB: { title: "In-Place Handover", desc: "Edit directly — screen never disappears" },
      response: "I've updated Northstar's annual pricing PDF and opened the cover note right beside you in the split-screen canvas. Go ahead and edit the personal note directly — once you click send, I'll deliver it and confirm receipt.",
      artifact: {
        title: "Live Renewal Note — You're in Control",
        meta: "Interactive split-screen · edit live",
        foot: "Edit directly above — Magnus waits for your click.",
        cta: "Send Proposal to Elena",
        doneCta: "✓ Delivered & confirmed",
        html: `
          <div class="art-card"><div><strong>Attached: Northstar_Q4_Annual_Plan.pdf</strong><span>15% volume tier applied · Q1 kickoff timeline included</span></div><span class="art-tag ok">Ready</span></div>
          <textarea class="art-editable" aria-label="Personal renewal note">Hi Elena — attached is your updated annual plan with the 15% volume tier applied. Excited to kick off Q1 together next month!</textarea>
        `
      }
    }
  };

  function setPresence(state, labelText) {
    orb.setState(state);
    presenceStage.classList.toggle("is-listening", state === "listening");
    presenceLabel.dataset.state = state;
    presenceLabel.textContent = labelText;
  }

  function sleep(ms) {
    return new Promise((r) => setTimeout(r, ms));
  }

  async function streamWords(el, text, token) {
    el.innerHTML = "";
    const words = text.split(" ");
    for (let i = 0; i < words.length; i++) {
      if (token !== runToken) return;
      const span = document.createElement("span");
      span.className = "stream-word";
      span.textContent = words[i];
      el.appendChild(span);
      await sleep(34);
    }
  }

  function buildCustomWorkflow(text) {
    return {
      prompt: text,
      steps: [
        "Understanding your goal & checking connected apps...",
        "Gathering relevant files and live web context...",
        "Verifying details before preparing deliverable...",
        "Assembling finished summary & next step..."
      ],
      doneSummary: "Completed 4 steps · 1.9s",
      tileA: { title: "Connected Apps", desc: "Checked relevant threads & project files" },
      tileB: { title: "Approval Ready", desc: "Result staged — nothing sent without you" },
      response: `Done — I gathered the latest context for "${text}", organized the key findings, and staged the finished deliverable in the split-screen canvas on the right for your one-click approval.`,
      artifact: {
        title: "Finished Deliverable",
        meta: "Interactive artifact · ready for review",
        foot: "Magnus waits for your OK before sending or publishing.",
        cta: "Approve & Share",
        doneCta: "✓ Approved & confirmed",
        html: `
          <div class="art-card"><div><strong>Request Summary</strong><span>${text.replace(/</g, "&lt;")}</span></div><span class="art-tag ok">Complete</span></div>
          <div class="art-card"><div><strong>Prepared Action</strong><span>Drafted cleanly in your team's tone with sources verified</span></div><span class="art-tag">Ready</span></div>
        `
      }
    };
  }

  async function runWorkflow(wf) {
    const token = ++runToken;
    activeWorkflow = wf;
    listening = false;
    micBtn.setAttribute("aria-pressed", "false");
    promptInput.value = wf.prompt;

    // Reset visuals
    workspaceStage.classList.remove("artifact-open");
    mainBox.classList.remove("is-executing", "is-done");
    tileA.classList.remove("is-executing", "is-done");
    tileB.classList.remove("is-executing", "is-done");
    reasoningPill.classList.remove("done");
    reasoningPill.classList.add("expanded");
    streamOutput.innerHTML = "";

    // Phase 1: Thinking & Planning (Shader shifts to vibrant violet/cyan)
    setPresence("thinking", "Thinking & planning...");
    reasoningStepsEl.innerHTML = wf.steps
      .map((s, i) => `<div class="r-step" id="r-step-${i}"><span>${s}</span><span></span></div>`)
      .join("");

    for (let i = 0; i < wf.steps.length; i++) {
      if (token !== runToken) return;
      reasoningSummary.textContent = wf.steps[i];
      const stepEl = document.getElementById(`r-step-${i}`);
      if (stepEl) stepEl.classList.add("active");

      // Phase 2: Tool Execution rotating conic-gradient borders
      if (i === 1) {
        setPresence("executing", "Working across your apps...");
        tileA.classList.add("is-executing");
        tileATitle.textContent = wf.tileA.title;
        tileADesc.textContent = "Fetching live data...";
      }
      if (i === 2) {
        tileA.classList.remove("is-executing");
        tileA.classList.add("is-done");
        tileADesc.textContent = wf.tileA.desc;
        tileB.classList.add("is-executing");
        tileBTitle.textContent = wf.tileB.title;
        tileBDesc.textContent = "Preparing deliverable...";
      }

      await sleep(520);
      if (token !== runToken) return;
      if (stepEl) {
        stepEl.classList.remove("active");
        stepEl.classList.add("complete");
        stepEl.lastElementChild.innerHTML = '<span class="check">✓</span>';
      }
    }

    tileB.classList.remove("is-executing");
    tileB.classList.add("is-done");
    tileBDesc.textContent = wf.tileB.desc;

    // Collapse accordion smoothly & stream masked tokens
    reasoningPill.classList.add("done");
    reasoningPill.classList.remove("expanded");
    reasoningSummary.textContent = wf.doneSummary;
    mainBox.classList.add("is-executing");

    // Populate and slide open split-screen Interactive Artifact Canvas
    artifactTitle.textContent = wf.artifact.title;
    artifactContent.innerHTML = wf.artifact.html;
    artifactFootNote.textContent = wf.artifact.foot;
    artifactActionBtn.textContent = wf.artifact.cta;
    artifactActionBtn.disabled = false;
    artifactMeta.textContent = wf.artifact.meta;
    workspaceStage.classList.add("artifact-open");

    await streamWords(streamOutput, wf.response, token);
    if (token !== runToken) return;

    mainBox.classList.remove("is-executing");
    setPresence("idle", "Ready · standing by");
  }

  // Accordion toggle
  reasoningToggle.addEventListener("click", () => {
    reasoningPill.classList.toggle("expanded");
  });

  // Artifact open/close & interactive approve button
  artifactBtn.addEventListener("click", () => workspaceStage.classList.add("artifact-open"));
  artifactCloseBtn.addEventListener("click", () => workspaceStage.classList.remove("artifact-open"));
  artifactActionBtn.addEventListener("click", () => {
    if (!activeWorkflow) return;
    artifactActionBtn.textContent = activeWorkflow.artifact.doneCta;
    artifactActionBtn.disabled = true;
    artifactFootNote.textContent = "Confirmed — Magnus checked the result and saved a receipt.";
  });

  // Prompt submission & chip shortcuts
  promptForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const val = promptInput.value.trim();
    if (!val) return;
    const match = Object.values(WORKFLOWS).find((w) => w.prompt.toLowerCase() === val.toLowerCase());
    runWorkflow(match || buildCustomWorkflow(val));
  });

  document.querySelectorAll(".chip-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      const wf = WORKFLOWS[btn.dataset.wf];
      if (wf) runWorkflow(wf);
    });
  });

  promptInput.addEventListener("focus", () => promptWrap.classList.add("active"));
  promptInput.addEventListener("blur", () => promptWrap.classList.remove("active"));

  // Voice / Listening mode toggle (triggers audio-reactive ripples + aqua/mint shader)
  micBtn.addEventListener("click", () => {
    listening = !listening;
    micBtn.setAttribute("aria-pressed", listening ? "true" : "false");
    if (listening) {
      setPresence("listening", "Listening · speak naturally...");
      promptInput.placeholder = "Listening — try \"Clear my morning inbox\"...";
    } else {
      setPresence("idle", "Ready · standing by");
      promptInput.placeholder = "Ask Magnus to handle something for your team...";
    }
  });

  // Download modal
  const dlg = document.getElementById("download-dialog");
  document.querySelectorAll("[data-open-download]").forEach((b) => b.addEventListener("click", () => dlg && dlg.showModal()));
  const closeDlg = document.getElementById("close-download-dialog");
  if (closeDlg && dlg) {
    closeDlg.addEventListener("click", () => dlg.close());
    dlg.addEventListener("click", (e) => { if (e.target === dlg) dlg.close(); });
  }

  // Auto-choreograph the first workflow cleanly on load
  runWorkflow(WORKFLOWS.inbox);
})();
