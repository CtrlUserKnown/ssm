(function () {
  var container = document.getElementById('changelog-content');
  if (!container) return;

  var url = 'https://raw.githubusercontent.com/CtrlUserKnown/ssm/main/CHANGELOG';

  fetch(url)
    .then(function (res) {
      if (!res.ok) throw new Error('Failed to fetch changelog');
      return res.text();
    })
    .then(function (text) {
      container.innerHTML = renderChangelog(text);
    })
    .catch(function () {
      container.innerHTML =
        '<p class="changelog-loading">Could not load changelog. ' +
        '<a href="https://github.com/CtrlUserKnown/ssm/blob/main/CHANGELOG" target="_blank" rel="noopener">View on GitHub</a></p>';
    });

  function renderChangelog(text) {
    var lines = text.split('\n');
    var html = '';
    var currentEntry = null;
    var currentSection = null;

    lines.forEach(function (line) {
      var trimmed = line.trim();
      if (!trimmed) return;

      // Version header: ## [1.0.0] - 2026-07-20
      var versionMatch = trimmed.match(/^## \[([^\]]+)\]\s*-\s*(.+)$/);
      if (versionMatch) {
        if (currentSection && currentEntry) currentEntry += '</ul></div>';
        if (currentEntry) html += '</div>' + currentEntry;
        currentEntry = '<div class="changelog-entry"><div class="changelog-version"><h2>' +
          esc(versionMatch[1]) + '</h2><span class="changelog-date">' +
          esc(versionMatch[2]) + '</span></div>';
        currentSection = null;
        return;
      }

      // Section header: ### Added, ### Changed, etc.
      var sectionMatch = trimmed.match(/^### (\w+)$/);
      if (sectionMatch && currentEntry) {
        if (currentSection) currentEntry += '</ul></div>';
        currentSection = sectionMatch[1].toLowerCase();
        currentEntry += '<div class="changelog-section"><h3><span class="changelog-badge changelog-badge--' +
          currentSection + '">' + esc(sectionMatch[1]) + '</span></h3><ul>';
        return;
      }

      // List item: - text
      if (trimmed.startsWith('- ') && currentSection) {
        currentEntry += '<li>' + renderInline(trimmed.slice(2)) + '</li>';
      }
    });

    if (currentSection && currentEntry) currentEntry += '</ul></div>';
    if (currentEntry) html += '</div>' + currentEntry;

    return html;
  }

  function renderInline(text) {
    // Bold
    text = text.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    // Inline code
    text = text.replace(/`([^`]+)`/g, '<code>$1</code>');
    return esc(text)
      .replace(/<strong>/g, '<strong>')
      .replace(/<\/strong>/g, '</strong>')
      .replace(/<code>/g, '<code>')
      .replace(/<\/code>/g, '</code>');
  }

  function esc(str) {
    var div = document.createElement('div');
    div.appendChild(document.createTextNode(str));
    return div.innerHTML;
  }
})();
