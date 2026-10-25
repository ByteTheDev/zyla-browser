// Zyla Browser chrome: the Arc-style sidebar (Spaces, favorites, pinned and regular tabs) or, with tabs on
// top, a tab strip above the toolbar. The host owns the tabs and sends the state; this page draws it and
// sends commands back.
// Bookmarks and history live in this page's localStorage, which the new-tab and history pages share.
(function () {
  'use strict';

  var HISTORY_KEY = 'tsgpui-browser.history';
  var ADDRESS_HISTORY_KEY = 'zyla.addressHistory';
  var BOOKMARKS_KEY = 'tsgpui-browser.bookmarks';
  var THEME_KEY = 'zyla.theme';
  var HISTORY_LIMIT = 500;
  var SIDEBAR_MIN = 200;
  var SIDEBAR_MAX = 420;

  var ICON_STOP = '<svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7l10 10M17 7L7 17"/></svg>';
  var ICON_RELOAD = '<svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/></svg>';
  var ICON_CLOSE = '<svg width="11" height="11" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"/></svg>';

  var $ = function (id) { return document.getElementById(id); };
  var app = $('app');
  var startupCaption = $('startup-caption');
  var spaceName = $('space-name');
  var favoritesEl = $('favorites');
  var pinnedEl = $('pinned');
  var regularEl = $('regular');
  var spacesEl = $('spaces');
  var tabstrip = $('tabstrip');
  var stripSpacesEl = $('strip-spaces');
  var stripTabsEl = $('strip-tabs');
  var settingsBtn = $('settings');
  var settingsPop = $('settings-pop');
  var vaultPanel = $('vault-panel');
  var vaultPasswordForm = $('password-form');
  var vaultCardForm = $('card-form');
  var vaultPasswordList = $('password-list');
  var vaultCardList = $('card-list');
  var vaultPasswordView = $('vault-password-view');
  var vaultCardView = $('vault-card-view');
  var extensionStatus = $('extension-status');
  var spaceConfirmEl = $('space-confirm');
  var spaceDeleteName = $('space-delete-name');
  var spaceDeleteCancel = $('space-delete-cancel');
  var spaceDeleteConfirm = $('space-delete-confirm');
  var menuEl = $('menu');
  var resizer = $('resizer');
  var sbToggle = $('sb-toggle');
  var sbOpen = $('sb-open');
  var addressEl = $('address');
  var addressForm = $('address-form');
  var addressCompletion = '';
  var addressCompletionPrefix = '';
  var readoutEl = $('readout');
  var lockEl = $('lock');
  var loadEl = $('load');
  var backEl = $('back');
  var forwardEl = $('forward');
  var reloadEl = $('reload');
  var homeEl = $('home');
  var starEl = $('star');
  var splitEl = $('split');
  var bookmarkBarEl = $('bookmark-bar');

  var state = { active: null, activeSpace: null, split: null, spaces: [], tabs: [], sidebar: { visible: true, width: 260, side: 'left' } };
  var wasLoading = {};
  var notice = null;
  var noticeTimer = null;
  var vaultData = { passwords: [], cards: [] };
  var pendingDeleteSpaceId = null;
  var spaceDeleteTrigger = null;
  var structureKey = '';
  var rowsById = {};
  var stripKey = '';
  var stripById = {};
  var resizing = false;
  var pendingWidth = null;
  var resizeFrame = 0;

  // ---- messages to the host -------------------------------------------------------------

  function send(message) {
    try {
      window.ipc.postMessage(JSON.stringify(message));
    } catch (error) {
      // Outside the browser host (for example in a plain browser preview) there is nowhere to send to.
    }
  }

  // ---- state helpers --------------------------------------------------------------------

  function activeTab() {
    for (var i = 0; i < state.tabs.length; i++) {
      if (state.tabs[i].id === state.active) return state.tabs[i];
    }
    return null;
  }

  function tabById(id) {
    for (var i = 0; i < state.tabs.length; i++) {
      if (state.tabs[i].id === id) return state.tabs[i];
    }
    return null;
  }

  // True for the two tabs shown side by side in split view.
  function inSplit(id) {
    return !!state.split && (state.split[0] === id || state.split[1] === id);
  }

  function currentSpace() {
    for (var i = 0; i < state.spaces.length; i++) {
      if (state.spaces[i].id === state.activeSpace) return state.spaces[i];
    }
    return null;
  }

  function isInternal(url) {
    return !url || /^(tsgpui:\/\/|https?:\/\/tsgpui\.localhost)/i.test(url);
  }

  function isWebUrl(url) {
    return /^(https?|file):/i.test(url || '') && !isInternal(url);
  }

  function tabTitle(tab) {
    if (tab.title) return tab.title;
    if (isInternal(tab.url)) return 'New tab';
    try { return new URL(tab.url).host || tab.url; } catch (error) { return tab.url; }
  }

  function letterFor(tab) {
    var title = tabTitle(tab).replace(/^[^A-Za-z0-9]+/, '');
    return (title.charAt(0) || '•').toUpperCase();
  }

  // Prefer the icon declared by the page, then try the conventional root favicon.
  function faviconNode(tab) {
    var wrap = document.createElement('span');
    wrap.className = 'fav';
    var letter = document.createElement('span');
    letter.className = 'fav-letter';
    letter.textContent = letterFor(tab);
    if (isWebUrl(tab.url)) {
      var img = document.createElement('img');
      img.alt = '';
      img.decoding = 'async';
      img.referrerPolicy = 'no-referrer';
      var fallback;
      try { fallback = new URL(tab.url).origin + '/favicon.ico'; } catch (error) { wrap.appendChild(letter); return wrap; }
      img.onerror = function () {
        if (img.src !== fallback) img.src = fallback;
        else wrap.replaceChildren(letter);
      };
      img.src = tab.favicon || fallback;
      wrap.appendChild(img);
    } else {
      wrap.appendChild(letter);
    }
    return wrap;
  }

  // ---- bookmarks and history --------------------------------------------------------------

  function readList(key) {
    try { return JSON.parse(localStorage.getItem(key)) || []; } catch (error) { return []; }
  }

  function writeList(key, list) {
    try { localStorage.setItem(key, JSON.stringify(list)); } catch (error) { /* storage full or disabled */ }
  }

  function isBookmarked(url) {
    return readList(BOOKMARKS_KEY).some(function (item) { return item.url === url; });
  }

  function toggleBookmark() {
    var tab = activeTab();
    if (!tab || !isWebUrl(tab.url)) {
      showNotice('Nothing to bookmark on this page');
      return;
    }
    var list = readList(BOOKMARKS_KEY);
    var index = -1;
    for (var i = 0; i < list.length; i++) if (list[i].url === tab.url) index = i;
    if (index >= 0) {
      list.splice(index, 1);
      showNotice('Bookmark removed');
    } else {
      list.unshift({ url: tab.url, title: tab.title || tab.url });
      showNotice('Bookmarked');
    }
    writeList(BOOKMARKS_KEY, list);
    renderBar();
  }

  function recordVisit(tab) {
    if (!isWebUrl(tab.url)) return;
    var list = readList(HISTORY_KEY);
    if (list.length && list[0].url === tab.url) {
      list[0].title = tab.title || list[0].title;
      list[0].at = Date.now();
    } else {
      list.unshift({ url: tab.url, title: tab.title || tab.url, at: Date.now() });
      if (list.length > HISTORY_LIMIT) list.length = HISTORY_LIMIT;
    }
    writeList(HISTORY_KEY, list);
  }

  function rememberAddress(text) {
    text = String(text || '').trim();
    if (!text) return;
    var list = readList(ADDRESS_HISTORY_KEY).filter(function (item) {
      return typeof item === 'string' && item.toLowerCase() !== text.toLowerCase();
    });
    list.unshift(text);
    if (list.length > HISTORY_LIMIT) list.length = HISTORY_LIMIT;
    writeList(ADDRESS_HISTORY_KEY, list);
  }

  function addressSuggestions(query) {
    var needle = query.trim().toLowerCase();
    if (!needle) return [];
    var candidates = readList(ADDRESS_HISTORY_KEY).slice();
    readList(HISTORY_KEY).forEach(function (visit) {
      if (!visit || typeof visit.url !== 'string') return;
      var value = visit.url;
      try {
        var parsed = new URL(value);
        if (/(^|\.)google\.com$/i.test(parsed.hostname) && parsed.pathname === '/search') {
          value = parsed.searchParams.get('q') || value;
        }
      } catch (error) { /* keep the stored URL as-is */ }
      candidates.push(value);
    });

    var seen = {};
    return candidates.filter(function (value) {
      if (typeof value !== 'string') return false;
      var normalized = value.trim();
      var key = normalized.toLowerCase();
      if (!normalized || seen[key] || key === needle || key.indexOf(needle) !== 0) return false;
      seen[key] = true;
      return true;
    }).slice(0, 1);
  }

  // A page that was loading and has now finished is a visit.
  function recordVisits() {
    var seen = {};
    state.tabs.forEach(function (tab) {
      seen[tab.id] = true;
      if (wasLoading[tab.id] === true && !tab.loading) recordVisit(tab);
      wasLoading[tab.id] = tab.loading;
    });
    Object.keys(wasLoading).forEach(function (id) {
      if (!seen[id]) delete wasLoading[id];
    });
  }

  function showNotice(text) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(function () {
      notice = null;
      renderBar();
    }, 3000);
    renderBar();
  }

  function vaultAction(label, action, id) {
    var button = document.createElement('button');
    button.type = 'button';
    button.textContent = label;
    button.dataset.action = action;
    button.dataset.id = String(id);
    return button;
  }

  function renderVault() {
    vaultPasswordList.textContent = '';
    vaultCardList.textContent = '';

    if (!vaultData.passwords.length) {
      var noPasswords = document.createElement('p');
      noPasswords.className = 'vault-empty';
      noPasswords.textContent = 'No saved passwords yet.';
      vaultPasswordList.appendChild(noPasswords);
    }
    vaultData.passwords.forEach(function (item) {
      var row = document.createElement('div');
      row.className = 'vault-item';
      var copy = document.createElement('div');
      copy.className = 'vault-item-copy';
      var title = document.createElement('div');
      title.className = 'vault-item-title';
      title.textContent = item.host;
      var subtitle = document.createElement('div');
      subtitle.className = 'vault-item-subtitle';
      subtitle.textContent = item.username;
      copy.append(title, subtitle);
      var actions = document.createElement('div');
      actions.className = 'vault-item-actions';
      actions.append(vaultAction('Fill', 'fill-password', item.id), vaultAction('Remove', 'delete-password', item.id));
      row.append(copy, actions);
      vaultPasswordList.appendChild(row);
    });

    if (!vaultData.cards.length) {
      var noCards = document.createElement('p');
      noCards.className = 'vault-empty';
      noCards.textContent = 'No saved cards yet.';
      vaultCardList.appendChild(noCards);
    }
    vaultData.cards.forEach(function (item) {
      var row = document.createElement('div');
      row.className = 'vault-item';
      var copy = document.createElement('div');
      copy.className = 'vault-item-copy';
      var title = document.createElement('div');
      title.className = 'vault-item-title';
      title.textContent = item.name;
      var subtitle = document.createElement('div');
      subtitle.className = 'vault-item-subtitle';
      subtitle.textContent = '•••• ' + item.lastFour + ' · Expires ' + String(item.expMonth).padStart(2, '0') + '/' + item.expYear;
      copy.append(title, subtitle);
      var actions = document.createElement('div');
      actions.className = 'vault-item-actions';
      actions.append(vaultAction('Fill', 'fill-card', item.id), vaultAction('Remove', 'delete-card', item.id));
      row.append(copy, actions);
      vaultCardList.appendChild(row);
    });
  }

  function openVault() {
    closeSettings();
    var tab = activeTab();
    if (tab && /^https:/i.test(tab.url || '')) {
      try { $('password-host').value = new URL(tab.url).hostname; } catch (error) { /* keep the field blank */ }
    }
    vaultPanel.hidden = false;
    renderVault();
    send({ cmd: 'get-vault' });
  }

  function closeVault() {
    vaultPanel.hidden = true;
  }

  // ---- sidebar ----------------------------------------------------------------------------

  // Rebuilds the rows only when their structure changes (titles, order, sections). Selection and
  // loading are updated in place, so the frequent load events cost almost nothing.
  function renderSidebar() {
    var space = state.activeSpace;
    var tabs = state.tabs.filter(function (tab) { return tab.space === space; });
    var favorites = tabs.filter(function (tab) { return tab.pinned && tab.favorite; });
    var pinned = tabs.filter(function (tab) { return tab.pinned && !tab.favorite; });
    var regular = tabs.filter(function (tab) { return !tab.pinned; });

    var key = JSON.stringify([space, tabs.map(function (tab) {
      return [tab.id, tabTitle(tab), tab.url, tab.favicon, tab.pinned, tab.favorite];
    })]);
    if (key !== structureKey) {
      structureKey = key;
      rowsById = {};
      fillContainer(favoritesEl, favorites, makeTile);
      fillContainer(pinnedEl, pinned, makeRow);
      fillContainer(regularEl, regular, makeRow);
      favoritesEl.hidden = favorites.length === 0;
      pinnedEl.hidden = pinned.length === 0;
    }
    tabs.forEach(function (tab) {
      var el = rowsById[tab.id];
      if (!el) return;
      var active = tab.id === state.active;
      el.classList.toggle('active', active);
      el.classList.toggle('loading', !!tab.loading);
      el.classList.toggle('split', inSplit(tab.id));
      el.setAttribute('aria-selected', active ? 'true' : 'false');
    });
  }

  function fillContainer(container, tabs, make) {
    container.textContent = '';
    tabs.forEach(function (tab) {
      var el = make(tab);
      rowsById[tab.id] = el;
      container.appendChild(el);
    });
  }

  function makeRow(tab) {
    var row = document.createElement('div');
    row.className = 'row';
    row.setAttribute('role', 'tab');
    row.tabIndex = 0;
    row.draggable = true;
    row.dataset.tab = String(tab.id);
    row.title = tabTitle(tab);
    var title = document.createElement('span');
    title.className = 'title';
    title.textContent = tabTitle(tab);
    var close = document.createElement('button');
    close.className = 'close';
    close.type = 'button';
    close.tabIndex = -1;
    close.setAttribute('aria-label', 'Close tab');
    close.innerHTML = ICON_CLOSE;
    row.append(faviconNode(tab), title, close);
    return row;
  }

  function makeTile(tab) {
    var tile = document.createElement('button');
    tile.className = 'tile';
    tile.type = 'button';
    tile.draggable = true;
    tile.dataset.tab = String(tab.id);
    tile.title = tabTitle(tab);
    tile.setAttribute('aria-label', tabTitle(tab));
    tile.appendChild(faviconNode(tab));
    return tile;
  }

  // The tab strip (tabs on top) shows the active Space's tabs, pinned ones first as in Chrome. Rebuilt only
  // when the structure changes, like the sidebar.
  function renderStrip() {
    var space = state.activeSpace;
    var tabs = state.tabs.filter(function (tab) { return tab.space === space; });
    var ordered = tabs.filter(function (tab) { return tab.pinned; })
      .concat(tabs.filter(function (tab) { return !tab.pinned; }));

    var key = JSON.stringify([space, ordered.map(function (tab) {
      return [tab.id, tabTitle(tab), tab.url, tab.favicon, tab.pinned];
    })]);
    if (key !== stripKey) {
      stripKey = key;
      stripById = {};
      stripTabsEl.textContent = '';
      ordered.forEach(function (tab) {
        var el = makeStripTab(tab);
        stripById[tab.id] = el;
        stripTabsEl.appendChild(el);
      });
    }
    ordered.forEach(function (tab) {
      var el = stripById[tab.id];
      if (!el) return;
      var active = tab.id === state.active;
      el.classList.toggle('active', active);
      el.classList.toggle('loading', !!tab.loading);
      el.classList.toggle('split', inSplit(tab.id));
      el.setAttribute('aria-selected', active ? 'true' : 'false');
    });
  }

  function makeStripTab(tab) {
    var el = document.createElement('div');
    el.className = 'ts-tab' + (tab.pinned ? ' pinned' : '');
    el.setAttribute('role', 'tab');
    el.draggable = true;
    el.dataset.tab = String(tab.id);
    el.dataset.section = tab.pinned ? 'pinned' : 'regular';
    el.title = tabTitle(tab);
    var title = document.createElement('span');
    title.className = 'ts-title';
    title.textContent = tabTitle(tab);
    var close = document.createElement('button');
    close.className = 'close';
    close.type = 'button';
    close.tabIndex = -1;
    close.setAttribute('aria-label', 'Close tab');
    close.innerHTML = ICON_CLOSE;
    el.append(faviconNode(tab), title, close);
    return el;
  }

  function renderSpaces() {
    drawSpaces(spacesEl, true);
    drawSpaces(stripSpacesEl, false);
  }

  function drawSpaces(container, deletable) {
    container.textContent = '';
    state.spaces.forEach(function (space) {
      var item = document.createElement('span');
      item.className = 'space-item';
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'space' + (space.id === state.activeSpace ? ' active' : '');
      button.dataset.space = String(space.id);
      button.title = space.name;
      button.setAttribute('aria-label', 'Switch to ' + space.name);
      button.textContent = (space.name.trim().charAt(0) || '•').toUpperCase();
      item.appendChild(button);

      if (deletable && state.spaces.length > 1) {
        var remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'space-remove';
        remove.dataset.spaceRemove = String(space.id);
        remove.title = 'Delete ' + space.name;
        remove.setAttribute('aria-label', 'Delete ' + space.name);
        remove.innerHTML = ICON_CLOSE;
        item.appendChild(remove);
      }
      container.appendChild(item);
    });
  }

  function applyLayout() {
    var sb = state.sidebar || { visible: true, width: 260 };
    var top = !!state.tabsOnTop;
    app.dataset.sidebar = sb.visible ? 'shown' : 'hidden';
    app.dataset.tabs = top ? 'top' : 'side';
    tabstrip.hidden = !top;
    document.documentElement.style.setProperty('--sb-w', sb.width + 'px');
    app.dataset.sidebarSide = sb.side === 'right' ? 'right' : 'left';
    sbOpen.hidden = top || sb.visible;
    var space = currentSpace();
    spaceName.textContent = space ? space.name : '';
    document.documentElement.dataset.tint = space ? space.tint : 'graphite';
  }

  // ---- tab actions: click, middle click, keyboard, drag and drop -------------------------

  function tabItems() {
    return Array.prototype.slice.call(document.querySelectorAll('#sidebar [data-tab]'));
  }

  function handleSidebarKey(event) {
    if (!$('sidebar').contains(document.activeElement)) return false;
    var focused = document.activeElement.closest ? document.activeElement.closest('[data-tab]') : null;
    if (!focused) return false;
    var items = tabItems();
    var index = items.indexOf(focused);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      var step = event.key === 'ArrowDown' ? 1 : -1;
      var next = items[(index + step + items.length) % items.length];
      if (next) next.focus();
      return true;
    }
    if (event.key === 'Enter' || event.key === ' ') {
      send({ cmd: 'select-tab', id: Number(focused.dataset.tab) });
      return true;
    }
    if (event.key === 'Delete' || event.key === 'Backspace') {
      send({ cmd: 'close-tab', id: Number(focused.dataset.tab) });
      return true;
    }
    if (event.key === 'Escape') {
      focused.blur();
      return true;
    }
    return false;
  }

  var dragId = null;

  function clearDropMarks() {
    document.querySelectorAll('.drop-before').forEach(function (el) { el.classList.remove('drop-before'); });
  }

  function openMenu(tab, x, y) {
    menuEl.textContent = '';
    var id = tab.id;
    var pinned = !!tab.pinned;
    var favorite = !!tab.favorite;
    var items = [{
      label: pinned ? 'Unpin tab' : 'Pin tab',
      run: function () { send({ cmd: 'set-pinned', id: id, pinned: !pinned, favorite: false }); }
    }];
    if (pinned) {
      items.push({
        label: favorite ? 'Remove from favorites' : 'Add to favorites',
        run: function () { send({ cmd: 'set-pinned', id: id, pinned: true, favorite: !favorite }); }
      });
    }
    items.push({ label: 'Close tab', run: function () { send({ cmd: 'close-tab', id: id }); } });
    items.forEach(function (item) {
      var button = document.createElement('button');
      button.type = 'button';
      button.setAttribute('role', 'menuitem');
      button.textContent = item.label;
      button.addEventListener('click', function () { closeMenu(); item.run(); });
      menuEl.appendChild(button);
    });
    menuEl.hidden = false;
    var width = menuEl.offsetWidth;
    var height = menuEl.offsetHeight;
    menuEl.style.left = Math.max(4, Math.min(x, window.innerWidth - width - 4)) + 'px';
    menuEl.style.top = Math.max(4, Math.min(y, window.innerHeight - height - 4)) + 'px';
  }

  function closeMenu() {
    menuEl.hidden = true;
  }

  function closeSettings() {
    settingsPop.hidden = true;
    settingsBtn.setAttribute('aria-expanded', 'false');
  }

  function openSpaceConfirm(space, trigger) {
    pendingDeleteSpaceId = space.id;
    spaceDeleteTrigger = trigger;
    spaceDeleteName.textContent = space.name;
    closeMenu();
    closeSettings();
    spaceConfirmEl.hidden = false;
    spaceDeleteCancel.focus();
  }

  function closeSpaceConfirm(restoreFocus) {
    spaceConfirmEl.hidden = true;
    pendingDeleteSpaceId = null;
    var trigger = spaceDeleteTrigger;
    spaceDeleteTrigger = null;
    if (restoreFocus && trigger && document.contains(trigger)) trigger.focus();
  }

  // ---- theme and settings ------------------------------------------------------------------

  function applyTheme() {
    var theme = localStorage.getItem(THEME_KEY) || 'system';
    document.documentElement.dataset.theme = theme;
    document.querySelectorAll('[data-theme-choice]').forEach(function (button) {
      button.setAttribute('aria-pressed', button.dataset.themeChoice === theme ? 'true' : 'false');
    });
  }

  window.addEventListener('storage', function (event) {
    if (event.key === THEME_KEY) applyTheme();
  });

  function markTabPosition() {
    var top = !!state.tabsOnTop;
    document.querySelectorAll('[data-layout-choice]').forEach(function (button) {
      var choice = button.dataset.layoutChoice === 'top';
      button.setAttribute('aria-pressed', choice === top ? 'true' : 'false');
    });
  }

  function markTint() {
    var space = currentSpace();
    var tint = space ? space.tint : 'graphite';
    document.querySelectorAll('[data-tint-choice]').forEach(function (button) {
      button.setAttribute('aria-pressed', button.dataset.tintChoice === tint ? 'true' : 'false');
    });
  }

  // ---- toolbar ----------------------------------------------------------------------------

  function focusAddress() {
    addressEl.focus();
    addressEl.select();
  }

  function updateAddressCompletion() {
    var typed = addressEl.value;
    addressCompletion = '';
    addressCompletionPrefix = '';
    var suggestion = addressSuggestions(typed)[0];
    if (!suggestion) {
      addressEl.setAttribute('aria-expanded', 'false');
      return;
    }
    addressCompletion = suggestion;
    addressCompletionPrefix = typed;
    addressEl.value = suggestion;
    addressEl.setSelectionRange(typed.length, suggestion.length);
    addressEl.setAttribute('aria-expanded', 'true');
  }

  function clearAddressCompletion() {
    addressCompletion = '';
    addressCompletionPrefix = '';
    addressEl.setAttribute('aria-expanded', 'false');
  }

  function renderBar() {
    var tab = activeTab();
    var loading = !!(tab && tab.loading);

    reloadEl.innerHTML = loading ? ICON_STOP : ICON_RELOAD;
    reloadEl.setAttribute('aria-label', loading ? 'Stop loading' : 'Reload');
    reloadEl.title = loading ? 'Stop (Esc)' : 'Reload (Ctrl+R)';
    loadEl.classList.toggle('on', loading);

    if (document.activeElement !== addressEl) {
      addressEl.value = tab && !isInternal(tab.url) ? tab.url : '';
    }
    lockEl.hidden = !(tab && /^https:/i.test(tab.url || ''));

    var bookmarked = !!(tab && isWebUrl(tab.url) && isBookmarked(tab.url));
    starEl.classList.toggle('on', bookmarked);
    starEl.setAttribute('aria-pressed', bookmarked ? 'true' : 'false');
    starEl.querySelector('path').setAttribute('fill', bookmarked ? 'currentColor' : 'none');
    renderBookmarkBar();

    var splitting = !!state.split;
    splitEl.classList.toggle('on', splitting);
    splitEl.setAttribute('aria-pressed', splitting ? 'true' : 'false');
    splitEl.title = splitting ? 'Close split view (Ctrl+\\)' : 'Split view (Ctrl+\\)';

    if (notice) {
      readoutEl.textContent = notice;
    } else if (tab && !loading && tab.loadMs !== null && tab.loadMs !== undefined) {
      readoutEl.textContent = tab.loadMs + ' ms';
    } else {
      readoutEl.textContent = '';
    }
  }

  function renderBookmarkBar() {
    var bookmarks = readList(BOOKMARKS_KEY).filter(function (item) {
      return item && typeof item.url === 'string' && isWebUrl(item.url);
    });
    bookmarkBarEl.replaceChildren();
    bookmarkBarEl.hidden = false;
    if (bookmarks.length === 0) {
      var empty = document.createElement('span');
      empty.className = 'bookmark-empty';
      empty.textContent = 'Bookmarks appear here';
      bookmarkBarEl.appendChild(empty);
    }
    bookmarks.forEach(function (bookmark) {
      var button = document.createElement('button');
      button.className = 'bookmark-item';
      button.type = 'button';
      button.title = bookmark.title || bookmark.url;
      button.setAttribute('aria-label', 'Open bookmark: ' + (bookmark.title || bookmark.url));
      var icon = document.createElement('span');
      icon.className = 'bookmark-favicon';
      var letter = document.createElement('span');
      letter.textContent = (bookmark.title || bookmark.url).replace(/^[^A-Za-z0-9]+/, '').charAt(0).toUpperCase() || '•';
      icon.appendChild(letter);
      try {
        var image = document.createElement('img');
        image.alt = '';
        image.referrerPolicy = 'no-referrer';
        image.src = new URL(bookmark.url).origin + '/favicon.ico';
        image.onerror = function () { icon.replaceChildren(letter); };
        icon.replaceChildren(image);
      } catch (error) { /* keep the initial letter */ }
      var label = document.createElement('span');
      label.className = 'bookmark-label';
      label.textContent = bookmark.title || bookmark.url;
      button.append(icon, label);
      button.addEventListener('click', function () { send({ cmd: 'navigate', text: bookmark.url }); });
      bookmarkBarEl.appendChild(button);
    });
  }

  function render() {
    applyLayout();
    renderSidebar();
    renderStrip();
    renderSpaces();
    renderBar();
    markTint();
    markTabPosition();
    applyTheme();
  }

  // ---- keys -------------------------------------------------------------------------------

  // Same keys as host/src/content.js, so the shortcuts work whichever part of the window has focus.
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
    var table = { t: 'new-tab', w: 'close-tab', l: 'focus-address', r: 'reload', d: 'bookmark', h: 'history',
      s: 'toggle-sidebar', '\\': 'split-view', '=': 'zoom-in', '+': 'zoom-in', '-': 'zoom-out', '0': 'zoom-reset' };
    return Object.prototype.hasOwnProperty.call(table, lower) ? table[lower] : null;
  }

  // Ctrl+1 to Ctrl+8 pick the tab at that place in the sidebar; Ctrl+9 picks the last one.
  function selectByNumber(number) {
    var items = tabItems();
    var item = number === 9 ? items[items.length - 1] : items[number - 1];
    if (item) send({ cmd: 'select-tab', id: Number(item.dataset.tab) });
  }

  function runCommand(name) {
    var digit = /^tab-([1-9])$/.exec(name);
    if (digit) selectByNumber(Number(digit[1]));
    else if (name === 'focus-address') focusAddress();
    else if (name === 'bookmark') toggleBookmark();
    else send({ cmd: name });
  }

  // ---- host messages ----------------------------------------------------------------------

  window.tsgpuiBrowser = {
    receive: function (message) {
      if (!message || typeof message !== 'object') return;
      if (message.type === 'state') {
        state = message;
        try { localStorage.setItem('zyla.tabsOnTop', message.tabsOnTop ? 'true' : 'false'); } catch (error) { /* storage unavailable */ }
        var extensionCount = Number(message.extensionsCount) || 0;
        extensionStatus.textContent = extensionCount === 0
          ? 'No local extensions installed.'
          : extensionCount + ' local extension' + (extensionCount === 1 ? '' : 's') + ' installed. Restart Zyla after adding one.';
        recordVisits();
        render();
        // Keep the startup screen above the tab WebViews until the host has created the restored views.
        // Pages can keep loading after this point; their own tab indicators show that work.
        if (document.body.dataset.startup !== 'ready') {
          var restoringTabs = Number(message.restoringTabs) || 0;
          if (startupCaption) {
            startupCaption.textContent = restoringTabs
              ? 'Preparing ' + restoringTabs + ' saved tab' + (restoringTabs === 1 ? '' : 's')
              : 'Opening your browser';
          }
          if (restoringTabs === 0) {
            document.body.dataset.startup = 'ready';
            send({ cmd: 'startup-complete' });
            var startupScreen = $('startup');
            if (startupScreen) {
              startupScreen.inert = true;
              startupScreen.setAttribute('aria-hidden', 'true');
              window.setTimeout(function () { startupScreen.remove(); }, 500);
            }
          }
        }
      } else if (message.type === 'vault-state') {
        vaultData = { passwords: message.passwords || [], cards: message.cards || [] };
        renderVault();
      } else if (message.type === 'command') {
        runCommand(message.name);
      } else if (message.type === 'notice') {
        showNotice(message.text);
      }
    }
  };

  // ---- events -----------------------------------------------------------------------------

  document.addEventListener('keydown', function (event) {
    if (!spaceConfirmEl.hidden) {
      if (event.key === 'Escape') {
        event.preventDefault();
        closeSpaceConfirm(true);
        return;
      }
      if (event.key === 'Tab') {
        event.preventDefault();
        if (event.shiftKey && document.activeElement === spaceDeleteCancel) spaceDeleteConfirm.focus();
        else spaceDeleteCancel.focus();
        return;
      }
      if (commandFor(event)) event.preventDefault();
      return;
    }
    if (handleSidebarKey(event)) {
      event.preventDefault();
      return;
    }
    var name = commandFor(event);
    if (!name) return;
    event.preventDefault();
    if (event.repeat) return;
    runCommand(name);
  }, true);

  addressForm.addEventListener('submit', function (event) {
    event.preventDefault();
    rememberAddress(addressEl.value);
    clearAddressCompletion();
    send({ cmd: 'navigate', text: addressEl.value });
    addressEl.blur();
  });

  addressEl.addEventListener('focus', function () { addressEl.select(); });
  addressEl.addEventListener('input', updateAddressCompletion);
  addressEl.addEventListener('blur', function () {
    clearAddressCompletion();
    renderBar();
  });
  addressEl.addEventListener('keydown', function (event) {
    if (event.key === 'Tab' && addressCompletion && addressEl.value === addressCompletion) {
      event.preventDefault();
      var end = addressEl.value.length;
      addressEl.setSelectionRange(end, end);
      clearAddressCompletion();
      return;
    }
    if (event.key === 'Escape') {
      if (addressCompletion) {
        addressEl.value = addressCompletionPrefix;
        addressEl.setSelectionRange(addressEl.value.length, addressEl.value.length);
        clearAddressCompletion();
      } else {
        addressEl.blur();
        renderBar();
      }
    }
  });

  backEl.addEventListener('click', function () { send({ cmd: 'back' }); });
  forwardEl.addEventListener('click', function () { send({ cmd: 'forward' }); });
  homeEl.addEventListener('click', function () { send({ cmd: 'home' }); });
  splitEl.addEventListener('click', function () { send({ cmd: 'split-view' }); });
  reloadEl.addEventListener('click', function () {
    var tab = activeTab();
    send({ cmd: tab && tab.loading ? 'stop' : 'reload' });
  });
  starEl.addEventListener('click', toggleBookmark);

  sbToggle.addEventListener('click', function () { send({ cmd: 'toggle-sidebar' }); });
  sbOpen.addEventListener('click', function () { send({ cmd: 'toggle-sidebar' }); });
  $('tabs-top').addEventListener('click', function () { send({ cmd: 'set-tabs-on-top', top: true }); });

  $('new-tab').addEventListener('click', function () { send({ cmd: 'new-tab' }); });
  $('new-space').addEventListener('click', function () { send({ cmd: 'new-space' }); });
  $('manage-vault').addEventListener('click', openVault);
  $('add-extension').addEventListener('click', function () {
    closeSettings();
    send({ cmd: 'pick-extension' });
  });

  $('vault-close').addEventListener('click', closeVault);
  vaultPanel.addEventListener('click', function (event) {
    if (event.target === vaultPanel) {
      closeVault();
      return;
    }
    var tab = event.target.closest ? event.target.closest('[data-vault-tab]') : null;
    if (tab) {
      var cards = tab.dataset.vaultTab === 'cards';
      vaultPasswordView.hidden = cards;
      vaultCardView.hidden = !cards;
      document.querySelectorAll('[data-vault-tab]').forEach(function (button) {
        button.setAttribute('aria-selected', button === tab ? 'true' : 'false');
      });
      return;
    }
    var action = event.target.closest ? event.target.closest('[data-action]') : null;
    if (!action) return;
    send({ cmd: action.dataset.action, id: Number(action.dataset.id) });
  });

  vaultPasswordForm.addEventListener('submit', function (event) {
    event.preventDefault();
    var form = new FormData(vaultPasswordForm);
    send({ cmd: 'save-password', host: form.get('host'), username: form.get('username'), password: form.get('password') });
    vaultPasswordForm.elements.password.value = '';
  });

  vaultCardForm.addEventListener('submit', function (event) {
    event.preventDefault();
    var form = new FormData(vaultCardForm);
    var expiry = String(form.get('expiry') || '').match(/^(\d{4})-(\d{2})$/);
    if (!expiry) return;
    send({ cmd: 'save-card', name: form.get('name'), number: form.get('number'), 'exp-year': Number(expiry[1]), 'exp-month': Number(expiry[2]) });
    vaultCardForm.elements.number.value = '';
  });

  // Clicks on a row, a tile or its close button.
  function tabFromEvent(event) {
    var item = event.target.closest ? event.target.closest('[data-tab]') : null;
    return item ? Number(item.dataset.tab) : null;
  }

  // The sidebar and the tab strip take the same clicks, middle clicks, context menu and drag and drop.
  function wireTabArea(area) {
    area.addEventListener('click', function (event) {
      var id = tabFromEvent(event);
      if (id === null) return;
      if (event.target.closest('.close')) send({ cmd: 'close-tab', id: id });
      else send({ cmd: 'select-tab', id: id });
    });

    area.addEventListener('auxclick', function (event) {
      if (event.button !== 1) return;
      var id = tabFromEvent(event);
      if (id !== null) send({ cmd: 'close-tab', id: id });
    });

    area.addEventListener('contextmenu', function (event) {
      var id = tabFromEvent(event);
      if (id === null) return;
      event.preventDefault();
      var tab = tabById(id);
      if (tab) openMenu(tab, event.clientX, event.clientY);
    });

    // Drag and drop: a tab can move within a section, between sections, and into the favorites grid.
    area.addEventListener('dragstart', function (event) {
      var item = event.target.closest ? event.target.closest('[data-tab]') : null;
      if (!item) return;
      dragId = item.dataset.tab;
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', dragId);
      item.classList.add('dragging');
    });

    area.addEventListener('dragend', function (event) {
      var item = event.target.closest ? event.target.closest('[data-tab]') : null;
      if (item) item.classList.remove('dragging');
      dragId = null;
      clearDropMarks();
    });

    area.addEventListener('dragover', function (event) {
      var section = event.target.closest ? event.target.closest('[data-section]') : null;
      if (!section) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = 'move';
      clearDropMarks();
      var target = event.target.closest ? event.target.closest('[data-tab]') : null;
      if (target && target.dataset.tab !== dragId) target.classList.add('drop-before');
    });

    area.addEventListener('drop', function (event) {
      var section = event.target.closest ? event.target.closest('[data-section]') : null;
      if (!section) return;
      event.preventDefault();
      clearDropMarks();
      var id = Number(event.dataTransfer.getData('text/plain'));
      if (!id) return;
      var target = event.target.closest ? event.target.closest('[data-tab]') : null;
      var before = target && Number(target.dataset.tab) !== id ? Number(target.dataset.tab) : null;
      send({ cmd: 'move-tab', id: id, before: before, section: section.dataset.section });
    });
  }

  wireTabArea($('sidebar'));
  wireTabArea(tabstrip);

  function onSpaceClick(event) {
    var remove = event.target.closest ? event.target.closest('[data-space-remove]') : null;
    if (remove) {
      var id = Number(remove.dataset.spaceRemove);
      var space = state.spaces.find(function (candidate) { return candidate.id === id; });
      if (space && state.spaces.length > 1) openSpaceConfirm(space, remove);
      return;
    }
    var button = event.target.closest ? event.target.closest('[data-space]') : null;
    if (button) send({ cmd: 'select-space', id: Number(button.dataset.space) });
  }

  spacesEl.addEventListener('click', onSpaceClick);
  stripSpacesEl.addEventListener('click', onSpaceClick);

  spaceDeleteCancel.addEventListener('click', function () { closeSpaceConfirm(true); });
  spaceDeleteConfirm.addEventListener('click', function () {
    var id = pendingDeleteSpaceId;
    var exists = state.spaces.some(function (space) { return space.id === id; });
    closeSpaceConfirm(false);
    if (id !== null && exists && state.spaces.length > 1) send({ cmd: 'delete-space', id: id });
  });
  spaceConfirmEl.addEventListener('click', function (event) {
    if (event.target === spaceConfirmEl) closeSpaceConfirm(true);
  });

  $('strip-new').addEventListener('click', function () { send({ cmd: 'new-tab' }); });
  $('strip-sidebar').addEventListener('click', function () { send({ cmd: 'set-tabs-on-top', top: false }); });

  // The sidebar settings control opens the dedicated settings tab.
  settingsBtn.addEventListener('click', function (event) {
    event.stopPropagation();
    send({ cmd: 'settings' });
  });

  settingsPop.addEventListener('click', function (event) {
    var tint = event.target.closest ? event.target.closest('[data-tint-choice]') : null;
    if (tint) {
      send({ cmd: 'set-tint', tint: tint.dataset.tintChoice });
      return;
    }
    var layout = event.target.closest ? event.target.closest('[data-layout-choice]') : null;
    if (layout) {
      send({ cmd: 'set-tabs-on-top', top: layout.dataset.layoutChoice === 'top' });
      return;
    }
    var theme = event.target.closest ? event.target.closest('[data-theme-choice]') : null;
    if (theme) {
      try { localStorage.setItem(THEME_KEY, theme.dataset.themeChoice); } catch (error) { /* storage unavailable */ }
      applyTheme();
    }
  });

  document.addEventListener('mousedown', function (event) {
    if (!menuEl.hidden && !menuEl.contains(event.target)) closeMenu();
    if (!settingsPop.hidden && !settingsPop.contains(event.target) && event.target !== settingsBtn) closeSettings();
  });

  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape') {
      closeMenu();
      closeSettings();
      closeVault();
    }
  });

  // Drag the space header to either window edge to dock the sidebar there.
  var sidebarDrag = null;
  var sidebarHead = $('sidebar').querySelector('.sb-head');
  sidebarHead.addEventListener('pointerdown', function (event) {
    if (event.button !== 0 || event.target.closest('button')) return;
    sidebarDrag = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, moved: false };
    sidebarHead.setPointerCapture(event.pointerId);
  });
  sidebarHead.addEventListener('pointermove', function (event) {
    if (!sidebarDrag || sidebarDrag.pointerId !== event.pointerId) return;
    if (Math.abs(event.clientX - sidebarDrag.x) > 5 || Math.abs(event.clientY - sidebarDrag.y) > 5) sidebarDrag.moved = true;
    if (sidebarDrag.moved) event.preventDefault();
  });
  sidebarHead.addEventListener('pointerup', function (event) {
    if (!sidebarDrag || sidebarDrag.pointerId !== event.pointerId) return;
    if (sidebarDrag.moved) {
      if (event.clientX >= window.innerWidth - 48) send({ cmd: 'sidebar-side', side: 'right' });
      else if (event.clientX <= 48) send({ cmd: 'sidebar-side', side: 'left' });
    }
    sidebarDrag = null;
  });
  sidebarHead.addEventListener('pointercancel', function () { sidebarDrag = null; });

  // Dragging the sidebar's edge to change its width. The layout follows each frame; the host hears the final width.
  function setSidebarWidth(width) {
    document.documentElement.style.setProperty('--sb-w', width + 'px');
  }

  function flushWidth() {
    resizeFrame = 0;
    if (pendingWidth !== null) send({ cmd: 'sidebar-width', width: pendingWidth });
  }

  resizer.addEventListener('pointerdown', function (event) {
    if (event.button !== 0) return;
    resizing = true;
    resizer.setPointerCapture(event.pointerId);
    event.preventDefault();
  });

  resizer.addEventListener('pointermove', function (event) {
    if (!resizing) return;
    var pointerWidth = app.dataset.sidebarSide === 'right' ? window.innerWidth - event.clientX : event.clientX;
    var width = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, pointerWidth));
    setSidebarWidth(width);
    pendingWidth = width;
    if (!resizeFrame) resizeFrame = requestAnimationFrame(flushWidth);
  });

  resizer.addEventListener('pointerup', function (event) {
    if (!resizing) return;
    resizing = false;
    resizer.releasePointerCapture(event.pointerId);
    if (pendingWidth !== null) send({ cmd: 'sidebar-width', width: pendingWidth });
    pendingWidth = null;
  });

  render();
  send({ cmd: 'ready' });
})();
