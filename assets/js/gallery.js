// Index page: show the pane named by the URL fragment, lay the page out so the
// media comes out largest, and filter the thumbnail list.
(function () {
  const body = document.body;
  const panes = document.querySelectorAll(".pane");
  const thumbs = document.querySelectorAll(".thumb");
  const filter = document.querySelector(".filter");
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
    for (const p of panes) {
      const on = p === target;
      p.classList.toggle("active", on);
      // A hidden video keeps playing its sound otherwise.
      if (!on) p.querySelectorAll("video").forEach((v) => v.pause());
    }
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

  // Videos have their own full-screen control; stills and the demo get a click.
  document.querySelectorAll(".media img, .media canvas").forEach((el) => {
    el.addEventListener("click", () => {
      if (document.fullscreenElement) document.exitFullscreen();
      else el.requestFullscreen?.();
    });
  });

  filter.addEventListener("input", () => {
    const q = filter.value.trim().toLowerCase();
    for (const t of thumbs) {
      t.parentElement.hidden = q !== "" && !t.textContent.toLowerCase().includes(q);
    }
  });

  window.addEventListener("hashchange", show);
  window.addEventListener("resize", layout);
  layout();
  show();
})();
