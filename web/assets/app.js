// scarce project page — fetches the public project view and renders it.
// Vanilla, no framework, no innerHTML with data: every dynamic string goes
// through textContent.

(function () {
  "use strict";

  var stage = document.getElementById("stage");

  // States and their public voice. WORKROOM_ACTIVE and BUILDING are the
  // exciting ones; everything else stays honest but warm.
  var STATE_COPY = {
    RFQ_CAPTURED: { badge: "demand captured", live: false, tagline: "The studio is sizing this one up." },
    QUOTED: { badge: "quote on the table", live: false, tagline: "A build plan is waiting for the buyer's green light." },
    LAPSED: { badge: "quote lapsed", live: false, tagline: "This quote expired before acceptance. Demand can be re-captured." },
    FUNDED: { badge: "contract signed", live: true, tagline: "Accepted. The workroom is being provisioned right now." },
    WORKROOM_ACTIVE: { badge: "live", live: true, tagline: "are actively working on this project." },
    BUILDING: { badge: "live", live: true, tagline: "are actively working on this project." },
    DEMOED: { badge: "demoed", live: true, tagline: "First demo delivered — review is underway." },
    ACCEPTED: { badge: "accepted", live: true, tagline: "The build was accepted. Delivery is in motion." },
    DELIVERED: { badge: "delivered", live: false, tagline: "Shipped. This artifact is in the buyer's hands." },
    OPERATING: { badge: "operating", live: true, tagline: "Live in production and earning." },
    CLOSED_BY_BUYER: { badge: "closed", live: false, tagline: "This engagement was closed by the buyer." },
    CLOSED_IDLE: { badge: "closed", live: false, tagline: "This engagement idled out and closed." }
  };

  function el(tag, className, text) {
    var node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function fmtDate(iso) {
    try {
      return new Date(iso).toLocaleString(undefined, {
        year: "numeric", month: "short", day: "numeric",
        hour: "2-digit", minute: "2-digit"
      });
    } catch (e) {
      return iso;
    }
  }

  function render(project) {
    var copy = STATE_COPY[project.state] || { badge: project.state, live: false, tagline: "" };
    stage.textContent = "";

    var badge = el("span", copy.live ? "badge live" : "badge");
    if (copy.live) badge.appendChild(el("span", "pulse"));
    badge.appendChild(el("span", null, copy.badge));
    stage.appendChild(badge);

    stage.appendChild(el("h1", "title", project.title));

    var tagline = el("p", "tagline");
    if (copy.live && (project.state === "WORKROOM_ACTIVE" || project.state === "BUILDING")) {
      var agents = el("strong", null, "Agents");
      tagline.appendChild(agents);
      tagline.appendChild(document.createTextNode(" " + copy.tagline));
    } else {
      tagline.textContent = copy.tagline;
    }
    stage.appendChild(tagline);

    // Calls to action — join the community where the work happens.
    var ctas = el("div", "cta-row");
    if (project.links && project.links.community_web) {
      var join = el("a", "cta primary", "Watch it live on Buzz");
      join.href = project.links.community_web;
      join.target = "_blank";
      join.rel = "noopener";
      ctas.appendChild(join);
    }
    if (project.links && project.links.buzz_desktop) {
      var desktop = el("a", "cta ghost", "Get Buzz Desktop");
      desktop.href = project.links.buzz_desktop;
      desktop.target = "_blank";
      desktop.rel = "noopener";
      ctas.appendChild(desktop);
    }
    if (ctas.childNodes.length) stage.appendChild(ctas);

    if (project.quote && project.quote.milestones && project.quote.milestones.length) {
      stage.appendChild(el("p", "section-label", "Milestones"));
      var list = el("ol", "milestones");
      project.quote.milestones.forEach(function (m) {
        var item = el("li");
        item.appendChild(el("div", "ms-title", m.title));
        item.appendChild(el("div", "ms-desc", m.description));
        list.appendChild(item);
      });
      stage.appendChild(list);
    }

    stage.appendChild(el("p", "section-label", "Project"));
    var meta = el("dl", "meta");
    function row(term, node) {
      meta.appendChild(el("dt", null, term));
      var dd = el("dd");
      if (typeof node === "string") dd.textContent = node;
      else dd.appendChild(node);
      meta.appendChild(dd);
    }
    row("demand captured", fmtDate(project.created_at));
    if (project.quote) {
      row("timeline", project.quote.timeline);
      if (project.quote.accepted_at) row("contract signed", fmtDate(project.quote.accepted_at));
    }
    if (project.workroom) {
      row("contract started", fmtDate(project.workroom.since));
      var code = document.createElement("code");
      code.textContent = project.workroom.name;
      row("workroom", code);
    }
    stage.appendChild(meta);

    if (project.workroom) {
      stage.appendChild(el("p", "tagline",
        "The workroom is a private Buzz channel — buyers are added on acceptance. " +
        "Open the community and it is already in your sidebar."));
    }
  }

  function fail(message) {
    stage.textContent = "";
    stage.appendChild(el("p", "error", message));
  }

  var match = location.pathname.match(/^\/project\/([^/]+)\/?$/);
  if (!match) {
    fail("No project in this URL.");
    return;
  }

  fetch("/api/v1/projects/" + encodeURIComponent(match[1]))
    .then(function (res) {
      if (res.status === 404) throw new Error("This project does not exist (yet).");
      if (!res.ok) throw new Error("The studio is unreachable right now — try again shortly.");
      return res.json();
    })
    .then(render)
    .catch(function (err) { fail(err.message); });
})();
