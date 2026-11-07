// Startup splash controller. Runs before first paint (classic script in
// <head>), so it can pick the intro mode before anything animates. The page's
// CSP blocks inline scripts, hence a separate file.
//
// Modes (mirrored from preferences into localStorage by the app):
//   full  – the whole moonrise, ~2.4 s minimum
//   short – compressed, ~0.9 s minimum
//   off   – no splash at all
// Reduce motion (app setting or the OS) shows the finished logo without
// movement and fades it out as soon as the app is ready.
(function () {
  "use strict";
  var root = document.documentElement;
  var read = function (k) {
    try { return window.localStorage.getItem(k); } catch (e) { return null; }
  };
  var mode = read("nocturn.intro");
  if (mode !== "full" && mode !== "short" && mode !== "off") mode = "full";
  var reduce = read("nocturn.reduceMotion") === "1";
  try {
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) reduce = true;
  } catch (e) { /* ignore */ }

  var MIN = { full: 2400, short: 900 };
  var start = performance.now();
  var appReady = false;
  var closing = false;
  var template = null;

  function apply(m, r) {
    root.setAttribute("data-intro", m);
    root.toggleAttribute("data-intro-still", !!r);
  }
  apply(mode, reduce);

  function el() { return document.getElementById("splash"); }

  function close(fast) {
    var s = el();
    if (!s || closing) return;
    closing = true;
    s.classList.add(fast ? "out-fast" : "out");
    var done = function () {
      if (s.parentNode) s.parentNode.removeChild(s);
      closing = false;
      root.removeAttribute("data-splash");
    };
    s.addEventListener("animationend", function (e) { if (e.target === s) done(); });
    setTimeout(done, 900); // in case animationend never fires
  }

  function maybeClose() {
    if (!appReady || !el()) return;
    var min = reduce ? 0 : MIN[mode] || 0;
    var wait = Math.max(0, min - (performance.now() - start));
    setTimeout(function () { close(false); }, wait);
  }

  function skip(e) {
    if (!el()) return;
    if (e && e.type === "keydown" && (e.key === "Control" || e.key === "Alt" || e.key === "Shift" || e.key === "Meta")) return;
    close(true);
  }

  function arm() {
    var s = el();
    if (!s) return;
    if (mode === "off") { s.parentNode.removeChild(s); return; }
    root.setAttribute("data-splash", "");
    if (!template) template = s.cloneNode(true);
    s.addEventListener("pointerdown", skip);
    // Failsafe: never hold the app behind the splash for long.
    setTimeout(function () { close(false); }, 15000);
  }

  window.addEventListener("keydown", function (e) { if (el()) skip(e); }, true);

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", arm);
  else arm();

  window.nocturnSplash = {
    /** Called by the app once preferences are loaded and the shell is drawn. */
    ready: function () { appReady = true; maybeClose(); },
    /** Keep the stored mode in step with preferences. */
    remember: function (m, r) {
      try {
        window.localStorage.setItem("nocturn.intro", m);
        window.localStorage.setItem("nocturn.reduceMotion", r ? "1" : "0");
      } catch (e) { /* ignore */ }
    },
    /** Replay the intro (Experience Studio preview). */
    preview: function (m, r) {
      if (!template || el()) return;
      if (m === "off") return;
      var prevMode = mode, prevReduce = reduce;
      mode = m; reduce = !!r || reduce;
      apply(mode, reduce);
      var s = template.cloneNode(true);
      document.body.appendChild(s);
      root.setAttribute("data-splash", "");
      s.addEventListener("pointerdown", skip);
      start = performance.now();
      appReady = true;
      maybeClose();
      // Restore the stored mode for the next real launch's attribute state.
      setTimeout(function () { mode = prevMode; reduce = prevReduce; }, (MIN[m] || 0) + 1200);
    },
  };
})();
