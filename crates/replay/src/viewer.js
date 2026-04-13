(function() {
  'use strict';
  var COLORS_HEX = ['#4a9eff','#ff4a6a','#4aff8a','#ffa04a','#c04aff','#4affd0','#ff4aff','#d0ff4a','#ff9090','#90d0ff','#d0d040','#40d0d0'];
  var COLORS_RGB = COLORS_HEX.map(function(h) {
    return [parseInt(h.slice(1,3),16), parseInt(h.slice(3,5),16), parseInt(h.slice(5,7),16)];
  });

  var R = REPLAY;
  var W = R.w, H = R.h;
  var totalCells = W * H;
  var KI = R.ki;
  var maxFrame = R.f.length - 1;

  // Grid state buffers
  var tiles = new Int8Array(totalCells);
  var own = new Int8Array(totalCells);
  var army = new Int32Array(totalCells);
  var loadedFrame = -1;

  function loadKeyframe(g) {
    for (var i = 0, j = 0; i < totalCells; i++, j += 3) {
      tiles[i] = g[j]; own[i] = g[j+1]; army[i] = g[j+2];
    }
  }
  function applyDelta(d) {
    for (var j = 0; j < d.length; j += 4) {
      var idx = d[j]; tiles[idx] = d[j+1]; own[idx] = d[j+2]; army[idx] = d[j+3];
    }
  }
  function applyFrame(f) { if (f.g) loadKeyframe(f.g); else if (f.d) applyDelta(f.d); }

  function seekTo(frameIdx) {
    if (frameIdx === loadedFrame) return;
    if (frameIdx === loadedFrame + 1) { applyFrame(R.f[frameIdx]); loadedFrame = frameIdx; return; }
    var kf = frameIdx - (frameIdx % KI);
    var startFrom;
    if (loadedFrame >= kf && loadedFrame < frameIdx) { startFrom = loadedFrame + 1; }
    else { applyFrame(R.f[kf]); startFrom = kf + 1; }
    for (var i = startFrom; i <= frameIdx; i++) applyFrame(R.f[i]);
    loadedFrame = frameIdx;
  }

  var canvas = document.getElementById('board');
  var ctx = canvas.getContext('2d');
  var statsEl = document.getElementById('stats');
  var scrubber = document.getElementById('scrubber');
  var turnLabel = document.getElementById('turn-label');
  var btnPlay = document.getElementById('btn-play');
  scrubber.max = maxFrame;

  var cellSize = 1;
  var showNumbers = false;

  function calcLayout() {
    var container = canvas.parentElement;
    var rect = container.getBoundingClientRect();
    var availW = rect.width - 4;
    var availH = rect.height - 4;
    var idealCs = Math.max(2, Math.min(availW / W, availH / H));
    cellSize = idealCs;
    var dpr = window.devicePixelRatio || 1;
    var cw = Math.round(W * idealCs);
    var ch = Math.round(H * idealCs);
    canvas.style.width = cw + 'px';
    canvas.style.height = ch + 'px';
    canvas.width = Math.round(cw * dpr);
    canvas.height = Math.round(ch * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  function brightness(count, maxA) {
    if (count <= 0) return 0.35;
    return 0.35 + 0.65 * Math.log1p(count) / Math.log1p(Math.max(maxA, 1));
  }

  function draw() {
    var cs = cellSize;
    var gap = cs >= 6 ? 1 : 0;
    var fill = cs - gap;
    ctx.fillStyle = '#141420';
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    var maxA = 1;
    for (var i = 0; i < totalCells; i++) { if (army[i] > maxA) maxA = army[i]; }

    for (var i = 0; i < totalCells; i++) {
      var col = i % W;
      var row = (i - col) / W;
      var x = col * cs;
      var y = row * cs;
      var t = tiles[i], o = own[i], a = army[i];

      if (t === 1) { ctx.fillStyle = '#3a3a4a'; ctx.fillRect(x, y, fill, fill); continue; }

      if (o >= 0) {
        var rgb = COLORS_RGB[o % COLORS_RGB.length];
        var br = t === 3 ? Math.max(brightness(a, maxA), 0.9) : brightness(a, maxA);
        ctx.fillStyle = 'rgb(' + (rgb[0]*br|0) + ',' + (rgb[1]*br|0) + ',' + (rgb[2]*br|0) + ')';
        ctx.fillRect(x, y, fill, fill);
        if (t === 2 && cs >= 5) {
          ctx.strokeStyle = 'rgba(255,255,255,0.6)'; ctx.lineWidth = 1;
          ctx.strokeRect(x+1.5, y+1.5, fill-3, fill-3);
        }
        if (t === 3 && cs >= 5) {
          var sz = Math.max(2, cs*0.3|0);
          ctx.fillStyle = 'rgba(255,215,0,0.9)';
          ctx.fillRect(x+fill-sz-1, y+1, sz, sz);
        }
      } else if (t === 2) {
        ctx.fillStyle = '#262638'; ctx.fillRect(x, y, fill, fill);
        if (cs >= 5) {
          ctx.strokeStyle = 'rgba(255,255,255,0.25)'; ctx.lineWidth = 1;
          ctx.strokeRect(x+1.5, y+1.5, fill-3, fill-3);
        }
      } else {
        ctx.fillStyle = '#1e1e2e'; ctx.fillRect(x, y, fill, fill);
      }
    }

    if (showNumbers && cs >= 14) {
      ctx.font = 'bold ' + Math.max(8, cs*0.45|0) + 'px monospace';
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle'; ctx.fillStyle = '#fff';
      for (var i = 0; i < totalCells; i++) {
        if (tiles[i] !== 1 && army[i] > 0) {
          var col = i % W, row = (i - col) / W;
          ctx.fillText(army[i], col*cs + (cs-1)/2, row*cs + (cs-1)/2);
        }
      }
    }
  }

  function renderStats(frameIdx) {
    var frame = R.f[frameIdx];
    var html = '';
    for (var j = 0; j < frame.s.length; j++) {
      var s = frame.s[j];
      var pid = s[0], land = s[1], arm = s[2], alive = s[3];
      var color = COLORS_HEX[pid % COLORS_HEX.length];
      var name = R.an[pid] || ('P' + pid);
      var cls = alive ? 'p-stat' : 'p-stat dead';
      html += '<div class="' + cls + '"><div class="p-dot" style="background:' + color + '"></div><span class="p-name">' + name + '</span><span class="p-val">' + land + 'L &middot; ' + arm + 'A</span></div>';
    }
    statsEl.innerHTML = html;
  }

  var currentFrame = 0, playing = false, speed = 10, lastTs = 0, accum = 0;

  function render(fi) {
    seekTo(fi); draw(); renderStats(fi);
    turnLabel.textContent = 'Turn ' + R.f[fi].t + ' / ' + R.f[maxFrame].t;
    scrubber.value = fi;
  }
  function setFrame(f) { currentFrame = Math.max(0, Math.min(f, maxFrame)); render(currentFrame); }
  function pauseAndSet(f) { playing = false; btnPlay.textContent = '\u25B6'; setFrame(f); }
  function togglePlay() {
    playing = !playing;
    btnPlay.textContent = playing ? '\u23F8' : '\u25B6';
    if (playing) { lastTs = performance.now(); accum = 0; requestAnimationFrame(tick); }
  }
  function tick(ts) {
    if (!playing) return;
    accum += ts - lastTs; lastTs = ts;
    var interval = 1000 / speed, advanced = false;
    while (accum >= interval && currentFrame < maxFrame) { currentFrame++; accum -= interval; advanced = true; }
    if (advanced) render(currentFrame);
    if (currentFrame >= maxFrame) { playing = false; btnPlay.textContent = '\u25B6'; return; }
    requestAnimationFrame(tick);
  }

  document.getElementById('btn-start').onclick = function() { pauseAndSet(0); };
  document.getElementById('btn-prev').onclick = function() { pauseAndSet(currentFrame - 1); };
  btnPlay.onclick = togglePlay;
  document.getElementById('btn-next').onclick = function() { pauseAndSet(currentFrame + 1); };
  document.getElementById('btn-end').onclick = function() { pauseAndSet(maxFrame); };
  scrubber.oninput = function(e) { pauseAndSet(parseInt(e.target.value)); };

  var speedBtns = document.querySelectorAll('.speed-btn[data-speed]');
  for (var i = 0; i < speedBtns.length; i++) {
    (function(btn) {
      btn.onclick = function() {
        speed = parseInt(btn.getAttribute('data-speed'));
        for (var j = 0; j < speedBtns.length; j++) speedBtns[j].className = 'speed-btn';
        btn.className = 'speed-btn active';
      };
    })(speedBtns[i]);
  }

  document.getElementById('btn-numbers').onclick = function() {
    showNumbers = !showNumbers;
    this.textContent = showNumbers ? '#' : '#\u0338';
    this.className = showNumbers ? 'speed-btn active' : 'speed-btn';
    render(currentFrame);
  };

  document.addEventListener('keydown', function(e) {
    if (e.target.tagName === 'INPUT') return;
    switch (e.key) {
      case ' ': e.preventDefault(); togglePlay(); break;
      case 'ArrowRight': pauseAndSet(currentFrame + 1); break;
      case 'ArrowLeft': pauseAndSet(currentFrame - 1); break;
      case 'Home': pauseAndSet(0); break;
      case 'End': pauseAndSet(maxFrame); break;
    }
  });

  window.addEventListener('resize', function() { calcLayout(); render(currentFrame); });
  calcLayout();
  render(0);
})();
