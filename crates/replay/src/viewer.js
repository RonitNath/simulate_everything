(function() {
  var COLORS = ["#4a9eff","#ff4a6a","#4aff8a","#ffa04a","#c04aff","#4affd0","#ff4aff","#d0ff4a"];
  var PLAY_CHAR = "\u25B6";
  var PAUSE_CHAR = "\u23F8";
  var NUMBERS_OFF = "#\u0338";

  var board = document.getElementById('board');
  var stats = document.getElementById('stats');
  var scrubber = document.getElementById('scrubber');
  var turnLabel = document.getElementById('turn-label');
  var btnPlay = document.getElementById('btn-play');

  var currentTurn = 0;
  var playing = false;
  var speed = 10;
  var showNumbers = false;
  var playTimer = null;
  var maxTurn = REPLAY.frames.length - 1;

  function hexToRgb(hex) {
    return [parseInt(hex.slice(1,3),16), parseInt(hex.slice(3,5),16), parseInt(hex.slice(5,7),16)];
  }

  function armyBrightness(count, maxArmy) {
    if (count <= 0) return 0.35;
    return 0.35 + 0.65 * Math.log1p(count) / Math.log1p(Math.max(maxArmy, 1));
  }

  function calcCellSize() {
    var maxW = (window.innerWidth - 320) * 0.9;
    var maxH = (window.innerHeight - 140) * 0.9;
    var availW = maxW - (REPLAY.width - 1);
    var availH = maxH - (REPLAY.height - 1);
    return Math.max(2, Math.min(Math.floor(availW / REPLAY.width), Math.floor(availH / REPLAY.height)));
  }

  // Pre-create cells.
  var cells = [];
  var cs = calcCellSize();
  board.style.gridTemplateColumns = "repeat(" + REPLAY.width + ", " + cs + "px)";
  board.style.gridTemplateRows = "repeat(" + REPLAY.height + ", " + cs + "px)";
  for (var i = 0; i < REPLAY.width * REPLAY.height; i++) {
    var div = document.createElement('div');
    div.className = 'c';
    board.appendChild(div);
    cells.push(div);
  }

  function render(turnIdx) {
    var frame = REPLAY.frames[turnIdx];
    if (!frame) return;

    var maxArmy = 1;
    var maxCityGarrison = 1;
    for (var i = 0; i < frame.grid.length; i++) {
      var cell = frame.grid[i];
      if (cell.armies > maxArmy) maxArmy = cell.armies;
      if (cell.tile === 'City' && cell.owner === null && cell.armies > maxCityGarrison) maxCityGarrison = cell.armies;
    }

    for (var i = 0; i < frame.grid.length; i++) {
      var cell = frame.grid[i];
      var div = cells[i];
      var isMtn = cell.tile === 'Mountain';
      var isCity = cell.tile === 'City';
      var isGen = cell.tile === 'General';
      var hasOwner = cell.owner !== null;

      var cls = 'c';
      var bg = '';
      var shadow = '';

      if (isMtn) {
        cls += ' c-mtn';
      } else if (hasOwner) {
        var t = armyBrightness(cell.armies, maxArmy);
        var rgb = hexToRgb(COLORS[cell.owner % COLORS.length]);
        var f = isGen ? Math.max(t, 0.9) : t;
        bg = 'rgb(' + Math.round(rgb[0]*f) + ',' + Math.round(rgb[1]*f) + ',' + Math.round(rgb[2]*f) + ')';
        if (isGen) {
          cls += ' c-gen';
          shadow = 'inset 0 0 0 2px rgba(255,215,0,0.9), 0 0 8px rgba(255,215,0,0.5)';
        } else if (isCity) {
          cls += ' c-city';
          shadow = 'inset 0 0 0 1px rgba(255,255,255,0.7)';
        }
      } else if (isCity) {
        var cityT = Math.min(cell.armies / Math.max(maxCityGarrison, 1), 1);
        var lum = 26 + 20 * (1 - cityT);
        bg = 'rgb(' + lum + ',' + lum + ',' + (lum + 12) + ')';
        cls += ' c-city';
        shadow = 'inset 0 0 0 1px rgba(255,255,255,' + (0.25 + 0.35*(1-cityT)).toFixed(2) + ')';
      } else {
        cls += ' c-empty';
      }

      div.className = cls;
      div.style.background = bg;
      div.style.boxShadow = shadow;

      if (showNumbers && !isMtn && cell.armies > 0) {
        div.innerHTML = '<span class="army">' + cell.armies + '</span>';
      } else {
        div.innerHTML = '';
      }
    }

    // Stats.
    var statsHtml = '';
    for (var j = 0; j < frame.stats.length; j++) {
      var s = frame.stats[j];
      var color = COLORS[s.player % COLORS.length];
      var name = REPLAY.agent_names[s.player];
      var statCls = s.alive ? 'p-stat' : 'p-stat dead';
      statsHtml += '<div class="' + statCls + '"><div class="p-dot" style="background:' + color + '"></div><span>' + name + '</span><span class="p-val">' + s.land + ' land &middot; ' + s.armies + ' army</span></div>';
    }
    stats.innerHTML = statsHtml;

    turnLabel.textContent = 'Turn ' + frame.turn + ' / ' + REPLAY.frames[maxTurn].turn;
    scrubber.value = turnIdx;
  }

  function setTurn(t) {
    currentTurn = Math.max(0, Math.min(t, maxTurn));
    render(currentTurn);
  }

  function stopTimer() {
    if (playTimer) { clearInterval(playTimer); playTimer = null; }
  }

  function startTimer() {
    stopTimer();
    var ms = Math.max(16, 1000 / speed);
    playTimer = setInterval(function() {
      if (currentTurn >= maxTurn) { playing = false; btnPlay.textContent = PLAY_CHAR; stopTimer(); return; }
      setTurn(currentTurn + 1);
    }, ms);
  }

  function togglePlay() {
    playing = !playing;
    btnPlay.textContent = playing ? PAUSE_CHAR : PLAY_CHAR;
    if (playing) startTimer(); else stopTimer();
  }

  function pauseAndSet(t) {
    playing = false;
    btnPlay.textContent = PLAY_CHAR;
    stopTimer();
    setTurn(t);
  }

  // Controls.
  document.getElementById('btn-start').onclick = function() { pauseAndSet(0); };
  document.getElementById('btn-prev').onclick = function() { pauseAndSet(currentTurn - 1); };
  btnPlay.onclick = togglePlay;
  document.getElementById('btn-next').onclick = function() { pauseAndSet(currentTurn + 1); };
  document.getElementById('btn-end').onclick = function() { pauseAndSet(maxTurn); };
  scrubber.oninput = function(e) { pauseAndSet(parseInt(e.target.value)); };

  // Speed buttons.
  var speedBtns = document.querySelectorAll('.speed-btn[data-speed]');
  for (var i = 0; i < speedBtns.length; i++) {
    (function(btn) {
      btn.onclick = function() {
        speed = parseInt(btn.getAttribute('data-speed'));
        for (var j = 0; j < speedBtns.length; j++) speedBtns[j].className = 'speed-btn';
        btn.className = 'speed-btn active';
        if (playing) startTimer();
      };
    })(speedBtns[i]);
  }

  // Numbers toggle.
  document.getElementById('btn-numbers').onclick = function() {
    showNumbers = !showNumbers;
    this.textContent = showNumbers ? '#' : NUMBERS_OFF;
    this.className = showNumbers ? 'speed-btn active' : 'speed-btn';
    render(currentTurn);
  };

  // Keyboard.
  document.addEventListener('keydown', function(e) {
    if (e.target.tagName === 'INPUT') return;
    switch(e.key) {
      case ' ': e.preventDefault(); togglePlay(); break;
      case 'ArrowRight': pauseAndSet(currentTurn + 1); break;
      case 'ArrowLeft': pauseAndSet(currentTurn - 1); break;
      case 'Home': pauseAndSet(0); break;
      case 'End': pauseAndSet(maxTurn); break;
    }
  });

  // Resize handling.
  window.addEventListener('resize', function() {
    var cs = calcCellSize();
    board.style.gridTemplateColumns = "repeat(" + REPLAY.width + ", " + cs + "px)";
    board.style.gridTemplateRows = "repeat(" + REPLAY.height + ", " + cs + "px)";
  });

  // Initial render.
  render(0);
})();
