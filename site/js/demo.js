(function () {
  'use strict';

  var container = document.getElementById('ssm-tui-demo');
  if (!container) return;

  var VERSION = '1.0.0';

  var sessions = [
    { name: 'production', host: 'prod.example.com', user: 'deploy', port: '22', tags: ['prod', 'web'], status: 'green', latency: 23, sparkline: [3,5,4,6,3,5,7,4,3,5,6,4,3,5] },
    { name: 'staging', host: 'staging.example.com', user: 'deploy', port: '2222', tags: ['staging'], status: 'green', latency: 45, sparkline: [5,8,6,10,7,5,8,9,6,5,7,8,6,5] },
    { name: 'dev-box', host: 'dev.internal.io', user: 'dev', port: '22', tags: ['dev'], status: 'yellow', latency: 156, sparkline: [8,12,15,10,18,20,12,8,15,20,18,12,10,8] },
    { name: 'db-primary', host: 'db.example.com', user: 'dba', port: '22', tags: ['db', 'critical'], status: 'green', latency: 12, sparkline: [2,3,2,4,2,3,2,3,4,2,3,2,3,2] },
    { name: 'monitoring', host: 'mon.internal.io', user: 'ops', port: '22', tags: ['ops'], status: 'red', latency: null, sparkline: [0,0,0,0,0,0,0,0,0,0,0,0,0,0] },
    { name: 'personal-vps', host: 'my.vps.cloud', user: 'root', port: '22', tags: ['personal'], status: 'green', latency: 89, sparkline: [10,12,15,11,13,14,10,12,15,11,13,10,12,14] },
    { name: 'ci-runner', host: 'ci.internal.io', user: 'runner', port: '22', tags: ['ci'], status: 'green', latency: 8, sparkline: [1,2,1,3,1,2,1,2,3,1,2,1,2,1] },
    { name: 'bastion', host: 'jump.example.com', user: 'admin', port: '22', tags: ['infra', 'jump'], status: 'green', latency: 34, sparkline: [4,5,6,4,5,4,5,6,4,5,4,5,6,4] }
  ];

  var themeOrder = ['auto', 'noir-cat', 'knew-pines', 'catppuccin', 'gruvbox', 'nord', 'tokyo-night'];
  var themes = {
    'auto':       { accent: '#89b4fa', bg: '#0e0e0e' },
    'noir-cat':   { accent: '#89b4fa', bg: '#0e0e0e' },
    'knew-pines': { accent: '#c4a7e7', bg: '#0e0e0e' },
    'catppuccin': { accent: '#89b4fa', bg: '#0e0e0e' },
    'gruvbox':    { accent: '#fabd2f', bg: '#0e0e0e' },
    'nord':       { accent: '#88c0d0', bg: '#0e0e0e' },
    'tokyo-night':{ accent: '#7aa2f7', bg: '#0e0e0e' }
  };

  var statusColors = { green: '#a6e3a1', yellow: '#f9e2af', red: '#f38ba8', gray: '#585b70' };
  var dimColor = '#585b70';
  var headerColor = '#89b4fa';

  var selectedIndex = 0;
  var currentTheme = 0;
  var searchMode = false;
  var searchQuery = '';
  var actionsOpen = false;
  var flashTimer = null;
  var keyBuffer = '';
  var keyTimer = null;
  var filteredIndices = sessions.map(function (_, i) { return i; });

  function escapeHtml(str) {
    return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function getStatusColor(status) {
    return statusColors[status] || statusColors.red;
  }

  function getStatusForLatency(latency) {
    if (latency === null) return 'red';
    if (latency < 50) return 'green';
    if (latency < 200) return 'yellow';
    return 'red';
  }

  function getAccentColor() {
    return themes[themeOrder[currentTheme]].accent;
  }

  function getThemeName() {
    return themeOrder[currentTheme];
  }

  function clamp(val, min, max) {
    return Math.max(min, Math.min(max, val));
  }

  function pad(str, len) {
    str = String(str);
    while (str.length < len) str += ' ';
    return str.substring(0, len);
  }

  function truncate(str, len) {
    if (str.length <= len) return str;
    return str.substring(0, len - 1) + '\u2026';
  }

  function truncateMiddle(str, len) {
    if (str.length <= len) return str;
    var half = Math.floor((len - 1) / 2);
    return str.substring(0, half) + '\u2026' + str.substring(str.length - half);
  }

  function highlightMatch(text, query) {
    if (!query) return escapeHtml(text);
    var escaped = escapeHtml(text);
    var lower = escaped.toLowerCase();
    var qLower = query.toLowerCase();
    var idx = lower.indexOf(qLower);
    if (idx === -1) return escaped;
    return escaped.substring(0, idx) +
      '<span class="ssm-highlight">' + escaped.substring(idx, idx + query.length) + '</span>' +
      escaped.substring(idx + query.length);
  }

  function buildSparkline(data, status) {
    var color = getStatusColor(status);
    var max = Math.max.apply(null, data.concat([1]));
    var bars = '';
    for (var i = 0; i < data.length; i++) {
      var h = Math.max(1, Math.round((data[i] / max) * 10));
      bars += '<span class="ssm-spark-bar" style="height:' + h + 'px;background:' + color + '"></span>';
    }
    return '<span class="ssm-sparkline">' + bars + '</span>';
  }

  function buildBorderLine(width) {
    return '\u2500'.repeat(width);
  }

  function buildHeaderBorder() {
    var w = 58;
    var title = ' ssm ';
    var ver = ' v' + VERSION + ' ';
    var chars = [];
    for (var i = 0; i < w; i++) chars.push('\u2500');
    for (var i = 0; i < title.length; i++) {
      var pos = 4 + i;
      if (pos < w) chars[pos] = title[i];
    }
    var verStart = w - ver.length - 2;
    for (var i = 0; i < ver.length; i++) {
      var pos = verStart + i;
      if (pos < w) chars[pos] = ver[i];
    }
    return chars.join('');
  }

  function renderRow(session, index) {
    var isSelected = index === selectedIndex;
    var cursor = isSelected ? '\u25B6 ' : '  ';

    var glyph, glyphColor;
    if (session.status === 'green') { glyph = '\u25CF'; glyphColor = statusColors.green; }
    else if (session.status === 'yellow') { glyph = '\u25CF'; glyphColor = statusColors.yellow; }
    else if (session.status === 'red') { glyph = '\u25CF'; glyphColor = statusColors.red; }
    else { glyph = '\u25CB'; glyphColor = statusColors.gray; }

    var hostFull = session.user + '@' + session.host;
    var nameStr = truncate(session.name, 18);
    var hostStr = truncateMiddle(hostFull, 22);
    var portStr = pad(session.port, 5);
    var latencyStr = session.latency !== null ? pad(session.latency + 'ms', 5) : pad('-', 5);

    var tagsStr = '';
    for (var t = 0; t < session.tags.length; t++) {
      tagsStr += escapeHtml(session.tags[t]) + ' ';
    }
    tagsStr = tagsStr.trimEnd();

    var sparkline = buildSparkline(session.sparkline, session.status);

    var row = document.createElement('div');
    row.className = 'ssm-row' + (isSelected ? ' ssm-row-selected' : '');
    row.dataset.index = index;

    row.innerHTML =
      '<span class="ssm-cursor">' + cursor + '</span>' +
      '<span class="ssm-glyph" style="color:' + glyphColor + '">' + glyph + ' </span>' +
      '<span class="ssm-cell-name">' + highlightMatch(nameStr, searchQuery) + '</span>' +
      '<span class="ssm-cell-host">' + highlightMatch(hostStr, searchQuery) + '</span>' +
      '<span class="ssm-cell-port">' + portStr + '</span>' +
      '<span class="ssm-cell-latency">' + latencyStr + '</span>' +
      '<span class="ssm-cell-spark">' + sparkline + '</span>' +
      '<span class="ssm-cell-tags">' + tagsStr + '</span>';

    return row;
  }

  function renderSessionList() {
    var list = container.querySelector('.ssm-list-body');
    if (!list) return;
    list.innerHTML = '';
    for (var i = 0; i < filteredIndices.length; i++) {
      var idx = filteredIndices[i];
      list.appendChild(renderRow(sessions[idx], idx));
    }
    scrollSelectedIntoView();
  }

  function scrollSelectedIntoView() {
    var list = container.querySelector('.ssm-list-body');
    if (!list) return;
    var selected = list.querySelector('.ssm-row-selected');
    if (selected) {
      selected.scrollIntoView({ block: 'nearest' });
    }
  }

  function updateSelection() {
    var list = container.querySelector('.ssm-list-body');
    if (!list) return;
    var rows = list.querySelectorAll('.ssm-row');
    for (var i = 0; i < rows.length; i++) {
      rows[i].classList.toggle('ssm-row-selected', parseInt(rows[i].dataset.index, 10) === selectedIndex);
    }
    scrollSelectedIntoView();
  }

  function showFlash(message) {
    var existing = container.querySelector('.ssm-flash');
    if (existing) existing.remove();
    if (flashTimer) clearTimeout(flashTimer);

    var flash = document.createElement('div');
    flash.className = 'ssm-flash';
    flash.textContent = '  ' + message;
    var desc = container.querySelector('.ssm-desc');
    if (desc) {
      desc.innerHTML = '';
      desc.appendChild(flash);
    }

    flashTimer = setTimeout(function () {
      renderDesc();
      flashTimer = null;
    }, 1800);
  }

  function renderDesc() {
    var desc = container.querySelector('.ssm-desc');
    if (!desc) return;
    desc.innerHTML = '<span class="ssm-desc-prompt">\u203A </span><span class="ssm-desc-text"></span>';
  }

  function showActionsMenu(session) {
    if (actionsOpen) closeActionsMenu();
    actionsOpen = true;

    var overlay = document.createElement('div');
    overlay.className = 'ssm-actions-overlay';
    overlay.id = 'ssm-actions-overlay';

    var menu = document.createElement('div');
    menu.className = 'ssm-actions-menu';
    menu.innerHTML =
      '<div class="ssm-actions-title">Actions for <strong>' + escapeHtml(session.name) + '</strong></div>' +
      '<div class="ssm-actions-item" data-action="connect"><span class="ssm-action-key">Enter</span> Connect</div>' +
      '<div class="ssm-actions-item" data-action="edit"><span class="ssm-action-key">e</span> Edit</div>' +
      '<div class="ssm-actions-item" data-action="delete"><span class="ssm-action-key">D</span> Delete</div>' +
      '<div class="ssm-actions-item" data-action="copy"><span class="ssm-action-key">y</span> Yank host</div>' +
      '<div class="ssm-actions-dismiss">Press <strong>Space</strong> or <strong>Esc</strong> to close</div>';

    overlay.appendChild(menu);
    container.appendChild(overlay);

    menu.addEventListener('click', function (e) {
      var item = e.target.closest('.ssm-actions-item');
      if (!item) return;
      executeAction(item.dataset.action, session);
      closeActionsMenu();
    });
  }

  function closeActionsMenu() {
    actionsOpen = false;
    var overlay = document.getElementById('ssm-actions-overlay');
    if (overlay) overlay.remove();
  }

  function executeAction(action, session) {
    switch (action) {
      case 'connect':
        showFlash('Connected to ' + session.name + ' (' + session.user + '@' + session.host + ')');
        break;
      case 'edit':
        showFlash('Editing ' + session.name + '...');
        break;
      case 'delete':
        showFlash('Deleted ' + session.name);
        break;
      case 'copy':
        showFlash('Copied ' + session.user + '@' + session.host);
        break;
    }
  }

  function showSearchBar() {
    searchMode = true;
    var footer = container.querySelector('.ssm-footer-keys');
    if (!footer) return;
    footer.innerHTML =
      '<span class="ssm-search-input-wrap">/<input class="ssm-search-input" id="ssm-search-input" type="text" autofocus autocomplete="off" spellcheck="false" value="' + escapeHtml(searchQuery) + '"></span>' +
      '<span class="ssm-footer-dim"> type to filter  enter apply  esc cancel</span>';
    var input = document.getElementById('ssm-search-input');
    if (input) {
      input.focus();
      input.addEventListener('input', function () {
        searchQuery = input.value;
        applyFilter();
      });
    }
  }

  function hideSearchBar() {
    searchMode = false;
    searchQuery = '';
    renderFooter();
    applyFilter();
  }

  function applyFilter() {
    if (!searchQuery) {
      filteredIndices = sessions.map(function (_, i) { return i; });
    } else {
      filteredIndices = [];
      for (var i = 0; i < sessions.length; i++) {
        var s = sessions[i];
        var haystack = (s.name + ' ' + s.host + ' ' + s.user + ' ' + s.tags.join(' ')).toLowerCase();
        if (haystack.indexOf(searchQuery.toLowerCase()) !== -1) {
          filteredIndices.push(i);
        }
      }
    }
    if (filteredIndices.indexOf(selectedIndex) === -1 && filteredIndices.length > 0) {
      selectedIndex = filteredIndices[0];
    }
    renderSessionList();
  }

  function renderFooter() {
    var footerKeys = container.querySelector('.ssm-footer-keys');
    if (!footerKeys) return;
    footerKeys.innerHTML =
      ' j/k move  enter connect  a add  e edit  / search  T tags  space menu  q quit ';
  }

  function buildStructure() {
    container.innerHTML = '';
    container.classList.add('ssm-tui');

    var header = document.createElement('div');
    header.className = 'ssm-border ssm-border--header';
    header.textContent = buildHeaderBorder();

    var colHeader = document.createElement('div');
    colHeader.className = 'ssm-col-header';
    colHeader.innerHTML =
      '<span class="ssm-col-cursor"></span>' +
      '<span class="ssm-col-glyph"></span>' +
      '<span class="ssm-col-name">NAME</span>' +
      '<span class="ssm-col-host">HOST</span>' +
      '<span class="ssm-col-port">PORT</span>' +
      '<span class="ssm-col-latency">PING</span>' +
      '<span class="ssm-col-spark"></span>' +
      '<span class="ssm-col-tags">TAGS</span>';

    var listBody = document.createElement('div');
    listBody.className = 'ssm-list-body';

    var desc = document.createElement('div');
    desc.className = 'ssm-desc';

    var borderFooter = document.createElement('div');
    borderFooter.className = 'ssm-border ssm-border--footer';
    borderFooter.textContent = buildBorderLine(58);

    var footerKeys = document.createElement('div');
    footerKeys.className = 'ssm-footer-keys';

    container.appendChild(header);
    container.appendChild(colHeader);
    container.appendChild(listBody);
    container.appendChild(desc);
    container.appendChild(borderFooter);
    container.appendChild(footerKeys);

    renderFooter();
    renderDesc();
    renderSessionList();
  }

  function handleKeyDown(e) {
    if (e.target.classList && e.target.classList.contains('ssm-search-input')) {
      if (e.key === 'Escape') {
        e.preventDefault();
        hideSearchBar();
      }
      return;
    }

    if (actionsOpen) {
      if (e.key === ' ' || e.key === 'Escape') {
        e.preventDefault();
        closeActionsMenu();
      }
      return;
    }

    if (e.key === '/') {
      e.preventDefault();
      showSearchBar();
      return;
    }

    if (e.key === 't') {
      e.preventDefault();
      currentTheme = (currentTheme + 1) % themeOrder.length;
      applyTheme();
      return;
    }

    if (e.key === 'j' || e.key === 'ArrowDown') {
      e.preventDefault();
      moveSelection(1);
      return;
    }

    if (e.key === 'k' || e.key === 'ArrowUp') {
      e.preventDefault();
      moveSelection(-1);
      return;
    }

    if (e.key === ' ') {
      e.preventDefault();
      if (filteredIndices.length > 0) {
        showActionsMenu(sessions[selectedIndex]);
      }
      return;
    }

    if (e.key === 'G') {
      e.preventDefault();
      if (filteredIndices.length > 0) {
        selectedIndex = filteredIndices[filteredIndices.length - 1];
        updateSelection();
      }
      return;
    }

    if (e.key === 'Enter') {
      e.preventDefault();
      if (filteredIndices.length > 0) {
        var s = sessions[selectedIndex];
        showFlash('Connected to ' + s.name + ' (' + s.user + '@' + s.host + ')');
      }
      return;
    }

    if (e.key === 'y') {
      e.preventDefault();
      if (filteredIndices.length > 0) {
        var s = sessions[selectedIndex];
        showFlash('Copied ' + s.user + '@' + s.host);
      }
      return;
    }

    clearTimeout(keyTimer);
    keyBuffer += e.key;
    keyTimer = setTimeout(function () { keyBuffer = ''; }, 500);

    if (keyBuffer === 'gg') {
      e.preventDefault();
      keyBuffer = '';
      if (filteredIndices.length > 0) {
        selectedIndex = filteredIndices[0];
        updateSelection();
      }
    }
  }

  function moveSelection(delta) {
    if (filteredIndices.length === 0) return;
    var pos = filteredIndices.indexOf(selectedIndex);
    if (pos === -1) pos = 0;
    pos = clamp(pos + delta, 0, filteredIndices.length - 1);
    selectedIndex = filteredIndices[pos];
    updateSelection();
  }

  function applyTheme() {
    var theme = themes[themeOrder[currentTheme]];
    container.style.setProperty('--ssm-accent', theme.accent);
    renderSessionList();
  }

  function animateSparklines() {
    for (var i = 0; i < sessions.length; i++) {
      var s = sessions[i];
      s.sparkline.shift();
      if (s.status !== 'red') {
        var last = s.sparkline[s.sparkline.length - 1] || 1;
        var delta = Math.floor(Math.random() * 5) - 2;
        s.sparkline.push(Math.max(1, last + delta));
      } else {
        s.sparkline.push(0);
      }
    }
    renderSessionList();
  }

  function probeSessions() {
    var idx = Math.floor(Math.random() * sessions.length);
    var s = sessions[idx];

    if (s.latency === null) {
      if (Math.random() > 0.7) {
        s.latency = Math.floor(Math.random() * 100) + 10;
      }
    } else {
      var jitter = Math.floor(Math.random() * 20) - 10;
      s.latency = Math.max(1, s.latency + jitter);
      if (Math.random() > 0.95) {
        s.latency = null;
      }
    }

    s.status = getStatusForLatency(s.latency);
    renderSessionList();
  }

  function init() {
    buildStructure();
    applyTheme();

    container.setAttribute('tabindex', '0');
    container.style.outline = 'none';
    container.focus();

    document.addEventListener('keydown', handleKeyDown);

    setInterval(animateSparklines, 2000);
    setInterval(probeSessions, 5000);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
