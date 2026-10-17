// Placeholder animation: a slowly rotating hue wheel with a triangle inscribed in it.
(function () {
  const canvas = document.getElementById("demo-canvas");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");

  function draw(t) {
    requestAnimationFrame(draw);
    if (!canvas.offsetParent) return; // its pane is hidden

    const w = canvas.width, h = canvas.height;
    const cx = w / 2, cy = h / 2, r = Math.min(w, h) * 0.45;
    const spin = t / 4000;

    ctx.clearRect(0, 0, w, h);

    for (let i = 0; i < 360; i++) {
      const a0 = (i / 360) * 2 * Math.PI + spin;
      const a1 = ((i + 1.5) / 360) * 2 * Math.PI + spin;
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      ctx.arc(cx, cy, r, a0, a1);
      ctx.fillStyle = `hsl(${i}, 100%, 50%)`;
      ctx.fill();
    }

    ctx.beginPath();
    for (let k = 0; k < 3; k++) {
      const a = -spin * 2 + (k * 2 * Math.PI) / 3;
      const pr = r * (0.75 + 0.1 * Math.sin(t / 700 + k));
      ctx.lineTo(cx + pr * Math.cos(a), cy + pr * Math.sin(a));
    }
    ctx.closePath();
    ctx.lineWidth = 3;
    ctx.strokeStyle = "white";
    ctx.stroke();
  }

  requestAnimationFrame(draw);
})();
