"use strict";

function el(tag, className, text) {
  var node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = String(text);
  return node;
}

function issueLabel(issue) {
  if (issue == null || issue === "") return "";
  return "#" + issue;
}

function newestFirst(events) {
  var copy = events.slice();
  var hasTime = copy.some(function (e) { return typeof e.ts === "number"; });
  if (hasTime) {
    copy.sort(function (a, b) {
      var ta = typeof a.ts === "number" ? a.ts : 0;
      var tb = typeof b.ts === "number" ? b.ts : 0;
      return tb - ta;
    });
  } else {
    copy.reverse();
  }
  return copy;
}

function normalizeItems(items) {
  if (!items) return [];
  if (Array.isArray(items)) return items.slice();
  return Object.keys(items).map(function (k) { return items[k]; });
}

function normalizeRooms(rooms) {
  if (!rooms) return [];
  if (Array.isArray(rooms)) return rooms;
  return Object.keys(rooms).map(function (k) {
    var v = rooms[k];
    if (v && typeof v === "object") return v;
    return { id: k, name: String(v) };
  });
}

function roomName(room) {
  if (room == null) return "";
  if (typeof room === "string") return room;
  return room.name || room.title || room.id || "";
}

function renderRooms(rooms) {
  var list = document.getElementById("rooms-list");
  var empty = document.getElementById("rooms-empty");
  list.textContent = "";
  if (!rooms.length) {
    empty.hidden = false;
    return;
  }
  empty.hidden = true;
  rooms.forEach(function (room) {
    var name = roomName(room);
    if (!name) return;
    list.appendChild(el("li", null, name));
  });
  if (!list.children.length) empty.hidden = false;
}

function renderWork(items) {
  var wrap = document.getElementById("work-list");
  var empty = document.getElementById("work-empty");
  wrap.textContent = "";
  if (!items.length) {
    empty.hidden = false;
    return;
  }
  empty.hidden = true;
  items.forEach(function (item) {
    var card = el("article", "card");
    var heading = item.title
      ? issueLabel(item.issue) + " — " + item.title
      : "Work " + issueLabel(item.issue);
    card.appendChild(el("h3", null, heading));

    var meta = el("p", "card-meta");
    if (item.owner) {
      var owner = el("span");
      owner.appendChild(document.createTextNode("Owner: "));
      owner.appendChild(el("strong", null, item.owner));
      meta.appendChild(owner);
    } else {
      meta.appendChild(el("span", null, "Owner: —"));
    }
    if (item.state) meta.appendChild(el("span", "state", item.state));
    card.appendChild(meta);

    if (Array.isArray(item.files) && item.files.length) {
      var files = el("ul", "files");
      item.files.forEach(function (f) {
        files.appendChild(el("li", null, f));
      });
      card.appendChild(files);
    }

    if (item.plan) {
      var plan = el("p", "plan", item.plan);
      card.appendChild(plan);
    }

    wrap.appendChild(card);
  });
}

function recordChoice(event, choice, note) {
  var payload = {
    type: "handoff",
    actor: "you",
    issue: event.issue,
    text: "Chose " + choice + (event.text ? " — " + event.text : "")
  };
  return fetch("/events", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload)
  }).then(function (res) {
    if (!res.ok) throw new Error("Could not save your choice. Please try again.");
    note.textContent = "You chose " + choice + ". Saved.";
  }).catch(function () {
    note.textContent = "Could not save your choice. Please try again.";
  });
}

function choiceLabel(option) {
  var key = String(option).toLowerCase();
  if (key === "allow") return "Allow";
  if (key === "edit") return "Edit";
  if (key === "stop") return "Stop";
  var s = String(option);
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function renderDecisions(timeline) {
  var wrap = document.getElementById("decision-list");
  var empty = document.getElementById("decision-empty");
  wrap.textContent = "";
  var decisions = timeline.filter(function (e) {
    return e && e.type === "decision";
  });
  if (!decisions.length) {
    empty.hidden = false;
    return;
  }
  empty.hidden = true;
  decisions.forEach(function (event) {
    var card = el("article", "card");
    var title = event.title || event.text || "A choice needs your answer";
    card.appendChild(el("h3", null, title));

    var meta = el("p", "card-meta");
    if (event.actor) meta.appendChild(el("span", null, event.actor));
    if (event.issue != null) meta.appendChild(el("span", null, issueLabel(event.issue)));
    card.appendChild(meta);

    if (event.text && event.title) {
      card.appendChild(el("p", null, event.text));
    }

    var options = Array.isArray(event.options) && event.options.length
      ? event.options
      : ["Allow", "Edit", "Stop"];

    var actions = el("div", "decision-actions");
    var note = el("p", "choice-note", "");
    note.setAttribute("role", "status");

    options.forEach(function (option) {
      var label = choiceLabel(option);
      var btnClass = label === "Allow" ? "btn-allow" : label === "Stop" ? "btn-stop" : "btn-edit";
      var btn = el("button", btnClass, label);
      btn.type = "button";
      btn.addEventListener("click", function () {
        var buttons = actions.querySelectorAll("button");
        buttons.forEach(function (b) { b.disabled = true; });
        recordChoice(event, label, note).finally(function () {
          buttons.forEach(function (b) { b.disabled = false; });
        });
      });
      actions.appendChild(btn);
    });

    card.appendChild(actions);
    card.appendChild(note);
    wrap.appendChild(card);
  });
}

function renderTimeline(timeline) {
  var list = document.getElementById("timeline-list");
  var empty = document.getElementById("timeline-empty");
  list.textContent = "";
  if (!timeline.length) {
    empty.hidden = false;
    return;
  }
  empty.hidden = true;
  timeline.forEach(function (event) {
    var li = el("li");
    var head = el("div", "timeline-head");
    head.appendChild(el("span", "actor", event.actor || "Someone"));
    if (event.type) head.appendChild(el("span", "kind", event.type));
    if (event.issue != null && event.issue !== "") {
      head.appendChild(el("span", "issue", issueLabel(event.issue)));
    }
    li.appendChild(head);
    if (event.text) li.appendChild(el("p", "timeline-text", event.text));
    list.appendChild(li);
  });
}

function render(data) {
  var rooms = normalizeRooms(data.rooms);
  var items = normalizeItems(data.items);
  items.sort(function (a, b) {
    return Number(a.issue || 0) - Number(b.issue || 0);
  });
  var timeline = newestFirst(Array.isArray(data.timeline) ? data.timeline : []);

  renderRooms(rooms);
  renderWork(items);
  renderDecisions(timeline);
  renderTimeline(timeline);

  var status = document.getElementById("status-line");
  status.textContent = items.length
    ? items.length + (items.length === 1 ? " work item" : " work items")
    : "Up to date";
}

function load() {
  fetch("/api/workspace")
    .then(function (res) {
      if (!res.ok) throw new Error("bad response");
      return res.json();
    })
    .then(function (data) {
      render(data || {});
    })
    .catch(function () {
      var status = document.getElementById("status-line");
      status.textContent = "Could not load the workspace. Please try again shortly.";
      document.getElementById("rooms-empty").hidden = false;
      document.getElementById("work-empty").hidden = false;
      document.getElementById("timeline-empty").hidden = false;
    });
}

document.addEventListener("DOMContentLoaded", load);
