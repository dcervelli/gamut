// Index page: show the pane named by the URL fragment, lay the page out so the
// media comes out largest, and filter the thumbnail list.
(function () {
  const body = document.body;
  const panes = document.querySelectorAll(".pane");
  const thumbs = document.querySelectorAll(".thumb");
  const filter = document.querySelector(".filter");
  const sections = document.querySelectorAll(".crumb .section");
  const groups = document.querySelectorAll(".crumb-group");
  const viewer = document.querySelector(".viewer");
  if (!panes.length) return;

  const css = getComputedStyle(document.documentElement);
  const px = (name) => parseFloat(css.getPropertyValue(name));

  // Try each placement of the list (left column or bottom strip) and the text
  // (right of the media or below it), and keep whichever gives the widest
  // media. Earlier entries win ties, which happen once the media hits its cap.
  function layout() {
    const W = window.innerWidth, H = window.innerHeight;
    const compact = H < px("--compact-below");
    const aspect = px("--media-aspect-w") / px("--media-aspect-h");
    const pad = px("--pad"), gap = px("--gap");
    const header = compact ? 0 : px("--header-h");

    let best = null;
    for (const nav of ["left", "bottom"]) {
      for (const cap of ["right", "below"]) {
        let w = W - 2 * pad, h = H - header - 2 * pad;
        if (nav === "left") w -= px("--sidebar-w");
        if (nav === "bottom") h -= px("--strip-h");
        if (cap === "right") w -= px("--caption-w") + gap;
        if (cap === "below") h -= px("--caption-h") + gap;
        const mw = Math.max(0, Math.min(w, h * aspect, px("--media-max-w")));
        if (!best || mw > best.mw + 0.5) best = { nav, cap, mw };
      }
    }

    body.classList.toggle("compact", compact);
    for (const n of ["left", "bottom"]) body.classList.toggle("nav-" + n, best.nav === n);
    for (const c of ["right", "below"]) body.classList.toggle("cap-" + c, best.cap === c);
    body.style.setProperty("--media-w", Math.floor(best.mw) + "px");
    body.style.setProperty("--media-h", Math.floor(best.mw / aspect) + "px");
  }

  function show() {
    const id = decodeURIComponent(location.hash.slice(1));
    const target = [...panes].find((p) => p.dataset.id === id) || panes[0];
    // Start a newly shown section at its top, not where the last one was left
    // (its next link sits at the bottom).
    const entering = !target.classList.contains("active");
    if (entering) {
      viewer.scrollTop = 0;
      target.querySelectorAll(".caption").forEach((c) => (c.scrollTop = 0));
    }
    for (const p of panes) {
      const on = p === target;
      p.classList.toggle("active", on);
      // Play the shown section's video; a hidden one keeps playing otherwise.
      // play() rejects if the browser blocks autoplay; the controls remain.
      p.querySelectorAll("video").forEach((v) => (on ? v.play().catch(() => {}) : v.pause()));
    }
    for (const c of cycles) {
      if (c.pane !== target) c.leave();
      else if (entering) c.enter();
    }
    for (const g of groups) g.textContent = target.dataset.group;
    for (const s of sections) s.textContent = target.dataset.title;
    document.title = [document.querySelector(".brand a").textContent, target.dataset.group, target.dataset.title].join(" / ");
    for (const t of thumbs) {
      const on = t.dataset.id === target.dataset.id;
      t.classList.toggle("active", on);
      if (on) {
        t.setAttribute("aria-current", "page");
        t.scrollIntoView({ block: "nearest", inline: "nearest" });
      } else {
        t.removeAttribute("aria-current");
      }
    }
  }

  // Videos have their own full-screen control; stills get a click.
  document.querySelectorAll(".media img").forEach((el) => {
    el.addEventListener("click", () => {
      if (document.fullscreenElement) document.exitFullscreen();
      else el.requestFullscreen?.();
    });
  });

  // A pane with data-cycle (seconds) steps through the #id links in its text:
  // the current link is highlighted, with a bar filling under it until the
  // next step, and the media shows the linked section's poster with a link
  // through to it. A link to the pane itself shows its own media. Hovering or
  // focusing a link takes over from the timer, which picks up again from that
  // link once the links have been left alone for resumeAfter ms; hovering the
  // media only pauses it, so its link can be clicked before it moves on.
  const resumeAfter = 15000;
  const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)");
  function makeCycle(pane) {
    const media = pane.querySelector(".media");
    const seconds = parseFloat(pane.dataset.cycle);
    if (!media || !(seconds > 0)) return null;
    pane.style.setProperty("--cycle", seconds + "s");

    const steps = [];
    for (const link of pane.querySelectorAll('.caption a[href^="#"]')) {
      if (link.closest(".pager")) continue;
      const id = decodeURIComponent(link.hash.slice(1));
      const target = [...panes].find((p) => p.dataset.id === id);
      if (!target) continue;
      let frame = null;
      if (target !== pane) {
        const src = target.querySelector("video")?.poster || target.querySelector(".media img")?.src;
        if (!src) continue; // a doc section has no picture to show
        frame = document.createElement("a");
        frame.className = "frame";
        frame.href = "#" + id;
        frame.tabIndex = -1; // the highlighted link goes to the same place
        frame.dataset.src = src;
        frame.innerHTML = '<img alt=""><span class="cta"><span class="go">Go to</span> <span class="to"></span></span>';
        frame.querySelector(".cta .to").textContent = target.dataset.title;
        media.append(frame);
      }
      link.classList.add("cycle-link");
      steps.push({ link, frame });
    }
    if (!steps.length) return null;

    let index = 0, timer = 0, resume = 0, manual = false, paused = false;
    function go(i) {
      index = i;
      steps.forEach((s, j) => {
        s.link.classList.toggle("lit", j === i);
        s.link.classList.remove("timing");
        s.frame?.classList.toggle("shown", j === i);
      });
      media.classList.toggle("on-base", !steps[i].frame);
      schedule();
    }
    function schedule() {
      clearTimeout(timer);
      const link = steps[index].link;
      link.classList.remove("timing");
      if (manual || paused || reduceMotion.matches || document.hidden || !pane.classList.contains("active")) return;
      void link.offsetWidth; // restart the bar's animation
      link.classList.add("timing");
      timer = setTimeout(() => go((index + 1) % steps.length), seconds * 1000);
    }

    steps.forEach((s, i) => {
      const takeOver = () => { clearTimeout(resume); manual = true; go(i); };
      s.link.addEventListener("pointerenter", (e) => e.pointerType === "mouse" && takeOver());
      s.link.addEventListener("focus", takeOver);
      s.link.addEventListener("pointerleave", release);
      s.link.addEventListener("blur", release);
    });
    function release() {
      clearTimeout(resume);
      resume = setTimeout(() => {
        // Still on one by keyboard (a mouse click's focus doesn't count).
        if (steps.some((s) => s.link.matches(":hover, :focus-visible"))) return;
        manual = false;
        schedule();
      }, resumeAfter);
    }
    media.addEventListener("pointerenter", (e) => { if (e.pointerType === "mouse") { paused = true; schedule(); } });
    media.addEventListener("pointerleave", () => { paused = false; schedule(); });
    document.addEventListener("visibilitychange", schedule);
    reduceMotion.addEventListener("change", schedule);

    return {
      pane,
      // Start over from the first link each time the pane is shown.
      enter() {
        for (const s of steps) {
          const img = s.frame?.querySelector("img");
          if (img && !img.getAttribute("src")) img.src = s.frame.dataset.src;
        }
        clearTimeout(resume);
        manual = paused = false;
        go(0);
      },
      leave: schedule,
    };
  }
  const cycles = [...panes].map(makeCycle).filter(Boolean);

  filter.addEventListener("input", () => {
    const q = filter.value.trim().toLowerCase();
    for (const t of thumbs) {
      t.parentElement.hidden = q !== "" && !t.textContent.toLowerCase().includes(q);
    }
    // Drop a group's heading along with its last match.
    for (const g of document.querySelectorAll(".sidebar .group")) {
      g.hidden = ![...g.querySelectorAll("li")].some((li) => !li.hidden);
    }
  });

  window.addEventListener("hashchange", show);
  window.addEventListener("resize", layout);
  layout();
  show();
})();
