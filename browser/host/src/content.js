// Injected into every page the browser shows. Browser shortcuts are caught here (the page has focus,
// so the chrome never sees them) and sent to the host as a navigation to tsgpui-cmd://<name>. The host
// cancels that navigation and runs the command. Pages get no other channel to the host.
(function () {
  if (window.top !== window) return;

  // Keep the browser chrome in sync with single-page apps that change the URL
  // without causing a WebView page-load event.
  var lastReportedUrl = location.href;
  function reportUrl() {
    if (location.href === lastReportedUrl || !/^https?:$/.test(location.protocol)) return;
    lastReportedUrl = location.href;
    location.href = 'tsgpui-cmd://url?value=' + encodeURIComponent(lastReportedUrl);
  }
  ['pushState', 'replaceState'].forEach(function (method) {
    var original = history[method];
    history[method] = function () {
      var result = original.apply(this, arguments);
      reportUrl();
      return result;
    };
  });
  window.addEventListener('popstate', reportUrl);
  window.addEventListener('hashchange', reportUrl);

  // Report the site's declared favicon to the browser chrome. Pages often use a non-root
  // icon path, so looking only for /favicon.ico misses many otherwise valid tab logos.
  var lastReportedFavicon = '';
  function reportFavicon() {
    var links = document.querySelectorAll('link[rel~="icon"], link[rel~="apple-touch-icon"]');
    var chosen = null;
    for (var i = 0; i < links.length; i++) {
      if (links[i].relList.contains('icon')) { chosen = links[i]; break; }
      if (!chosen) chosen = links[i];
    }
    var href = chosen && chosen.href;
    if (!href || href === lastReportedFavicon || !/^https?:/i.test(href)) return;
    lastReportedFavicon = href;
    location.href = 'tsgpui-cmd://favicon?value=' + encodeURIComponent(href);
  }
  document.addEventListener('DOMContentLoaded', reportFavicon, { once: true });
  new MutationObserver(reportFavicon).observe(document, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ['href', 'rel']
  });

  var CTRL = {
    t: 'new-tab', w: 'close-tab', l: 'focus-address', r: 'reload', d: 'bookmark', h: 'history', s: 'toggle-sidebar',
    '\\': 'split-view',
    '=': 'zoom-in', '+': 'zoom-in', '-': 'zoom-out', '0': 'zoom-reset'
  };

  function commandFor(event) {
    var key = event.key;
    if (key === 'F5') return 'reload';
    if (key === 'F6') return 'focus-address';
    if (key === 'F12') return 'devtools';
    if (event.altKey && key === 'ArrowLeft') return 'back';
    if (event.altKey && key === 'ArrowRight') return 'forward';
    if (!(event.ctrlKey || event.metaKey)) return null;
    if (key === 'Tab') return event.shiftKey ? 'prev-tab' : 'next-tab';
    if (key === 'PageDown') return 'next-tab';
    if (/^[1-9]$/.test(key)) return 'tab-' + key;
    if (key === 'PageUp') return 'prev-tab';
    var lower = key.toLowerCase();
    if (event.shiftKey) {
      if (lower === 't') return 'reopen-tab';
      if (lower === 'i') return 'devtools';
      if (lower === 'r') return 'reload';
      return null;
    }
    return Object.prototype.hasOwnProperty.call(CTRL, lower) ? CTRL[lower] : null;
  }

  window.addEventListener('keydown', function (event) {
    var name = commandFor(event);
    if (!name) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.repeat) return;
    location.href = 'tsgpui-cmd://' + name;
  }, true);
})();
