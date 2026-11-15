(function () {
  const CATEGORY_ID = 'wol-imported-publications';
  const BOOK_PREFIX = 'wol-imported-book-';
  const MAP_STORAGE_KEY = 'wolImportedCategoryMap';

  const SLUG_TO_CATEGORY = {
    'pautas-y-manuales': 'manual',
    'manuals-and-guidelines': 'manual',
    'libros': 'bk',
    'books': 'bk',
    'folletos': 'brch',
    'brochures-and-booklets': 'brch',
    'tratados': 'trct',
    'tracts': 'trct',
    'la-atalaya': 'w',
    'watchtower': 'w',
    'despertad': 'g',
    'awake': 'g',
    'guía-de-actividades': 'mwb',
    'meeting-workbooks': 'mwb',
    'ministerio-del-reino': 'km',
    'kingdom-ministry': 'km',
    'biblias': 'bi',
    'bibles': 'bi',
    'glosario': 'dict',
    'glossary': 'dict',
    'perspicacia': 'it',
    'insight': 'it',
    'índices': 'dx',
    'index': 'dx',
    'catálogo-de-artículos': 'articleseries',
    'article-series': 'articleseries',
  };

  function normalizePath(pathname) {
    try {
      return decodeURIComponent(pathname).replace(/\/+$/, '').toLowerCase();
    } catch {
      return pathname.replace(/\/+$/, '').toLowerCase();
    }
  }

  function getPageLocale() {
    const seg = location.pathname.split('/').filter(Boolean)[0];
    return seg ? seg.toLowerCase() : (document.documentElement.lang || '').toLowerCase();
  }

  function getDirectoryList() {
    return document.querySelector('#article ul.directory.navCard, #materialNav ul.directory.navCard, ul.directory.navCard');
  }

  function isLibraryPage() {
    return /\/wol\/library\/r\d+\/lp-[a-z0-9]+\//i.test(location.pathname) && Boolean(getDirectoryList());
  }

  function hasBackNav() {
    return Boolean(document.querySelector('#materialNav a.backNav, .materialNav a.backNav'));
  }

  function extractCategoryFromElement(el) {
    if (!el) return null;
    for (const cls of el.classList) {
      if (cls.startsWith('cat-')) return cls.slice(4).toLowerCase();
    }
    for (const cls of el.classList) {
      if (cls.startsWith('icon-') && cls !== 'icon-default') return cls.slice(5).toLowerCase();
    }
    return null;
  }

  function loadCategoryMap() {
    try {
      return JSON.parse(sessionStorage.getItem(MAP_STORAGE_KEY) || '{}');
    } catch {
      return {};
    }
  }

  function saveCategoryMap(map) {
    try {
      sessionStorage.setItem(MAP_STORAGE_KEY, JSON.stringify(map));
    } catch {
      // Ignore storage quota / privacy errors
    }
  }

  function bookCategories(book) {
    const set = new Set();
    if (typeof book.category === 'string' && book.category.trim()) {
      set.add(book.category.trim().toLowerCase());
    }
    if (Array.isArray(book.categories)) {
      for (const c of book.categories) {
        if (typeof c === 'string' && c.trim()) set.add(c.trim().toLowerCase());
      }
    }
    return set;
  }

  function matchesLocale(book, pageLocale) {
    if (!pageLocale || !book.locale) return true;
    return book.locale.toLowerCase() === pageLocale;
  }

  function navigateCard(link) {
    link.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      window.location.assign(link.href);
    });
  }

  function createFallbackCategoryCard(count, isSpanish) {
    const label = isSpanish ? 'Publicaciones importadas' : 'Imported publications';
    const li = document.createElement('li');
    li.id = CATEGORY_ID;
    li.className = 'row card';
    li.setAttribute('role', 'presentation');

    const link = document.createElement('a');
    link.href = `/imported/?from=${encodeURIComponent(location.pathname)}`;
    link.className = 'jwac showRuby chrome cardContainer noTooltips';
    navigateCard(link);

    const thumb = document.createElement('div');
    thumb.className = 'cardThumbnail';
    const icon = document.createElement('span');
    icon.className = 'cardThumbnailImage thumbnail container jwf-jw-icons-all jwi-book-stack jwf-jw-icons-all icon-bk';
    thumb.appendChild(icon);

    const titleBlock = document.createElement('div');
    titleBlock.className = 'cardTitleBlock';
    const line1 = document.createElement('div');
    line1.className = 'cardLine1 ellipsized cardSingleLine';
    const secIcon = document.createElement('span');
    secIcon.className = 'sectionIcon';
    line1.appendChild(secIcon);
    line1.appendChild(document.createTextNode(` ${label} `));
    const hidden = document.createElement('div');
    hidden.className = 'hidden';
    titleBlock.appendChild(line1);
    titleBlock.appendChild(hidden);

    const detail = document.createElement('div');
    detail.className = 'cardTitleDetail';
    detail.textContent = String(count);

    const chevron = document.createElement('div');
    chevron.className = 'cardChevron';
    const chevIcon = document.createElement('div');
    chevIcon.className = 'icon';
    chevron.appendChild(chevIcon);

    link.appendChild(thumb);
    link.appendChild(titleBlock);
    link.appendChild(detail);
    link.appendChild(chevron);
    li.appendChild(link);
    return li;
  }

  function createBookCard(book, pageCategory) {
    const li = document.createElement('li');
    li.id = `${BOOK_PREFIX}${book.id}`;
    li.className = 'row card';
    li.setAttribute('role', 'presentation');

    const link = document.createElement('a');
    link.href = `/imported/${encodeURIComponent(book.id)}/index?from=${encodeURIComponent(location.pathname)}`;
    link.className = 'jwac showRuby chrome cardContainer noTooltips lnk';
    navigateCard(link);

    const thumb = document.createElement('div');
    thumb.className = 'cardThumbnail';
    if (book.coverDataUrl) {
      const img = document.createElement('img');
      img.className = 'cardThumbnailImage thumbnail document icon-default publication';
      img.src = book.coverDataUrl;
      img.alt = '';
      thumb.appendChild(img);
    } else {
      const icon = document.createElement('span');
      icon.className = `cardThumbnailImage thumbnail document jwf-jw-icons-all icon-${pageCategory || 'bk'} publication`;
      thumb.appendChild(icon);
    }

    const titleBlock = document.createElement('div');
    titleBlock.className = 'cardTitleBlock';
    const line1 = document.createElement('div');
    line1.className = 'cardLine1 ellipsized cardSingleLine';
    const secIcon = document.createElement('span');
    secIcon.className = 'sectionIcon';
    line1.appendChild(secIcon);
    line1.appendChild(document.createTextNode(` ${book.title} (${book.symbol}) `));
    const hidden = document.createElement('div');
    hidden.className = 'hidden';
    titleBlock.appendChild(line1);
    titleBlock.appendChild(hidden);

    const detail = document.createElement('div');
    detail.className = 'cardTitleDetail';
    detail.textContent = book.year > 0 ? String(book.year) : '';

    const chevron = document.createElement('div');
    chevron.className = 'cardChevron';
    const chevIcon = document.createElement('div');
    chevIcon.className = 'icon';
    chevron.appendChild(chevIcon);

    link.appendChild(thumb);
    link.appendChild(titleBlock);
    link.appendChild(detail);
    link.appendChild(chevron);
    li.appendChild(link);
    return li;
  }

  function detectSubpageCategory(list) {
    const map = loadCategoryMap();
    const mapped = map[normalizePath(location.pathname)];
    if (mapped) return mapped;

    const existingThumb = list.querySelector('li.row.card:not([id^="wol-imported-"]) .cardThumbnailImage');
    const fromDom = extractCategoryFromElement(existingThumb);
    if (fromDom) return fromDom;

    const lastSeg = normalizePath(location.pathname).split('/').pop() || '';
    return SLUG_TO_CATEGORY[lastSeg] || null;
  }

  async function syncImportedLibrary() {
    if (!isLibraryPage()) return;
    const list = getDirectoryList();
    if (!list) return;

    const invoke = window.__TAURI_INTERNALS__?.invoke;
    if (!invoke) return;

    try {
      const books = await invoke('list_imported_publications');
      if (!Array.isArray(books) || !books.length) return;

      const pageLocale = getPageLocale();
      const isSpanish = pageLocale === 'es';

      if (!hasBackNav()) {
        const availableCategories = new Set();
        const map = loadCategoryMap();
        for (const item of list.querySelectorAll('li.row.card')) {
          if (item.id === CATEGORY_ID) continue;
          const a = item.querySelector('a[href]');
          const cat = extractCategoryFromElement(item.querySelector('.cardThumbnailImage'));
          if (cat) {
            availableCategories.add(cat);
            if (a) {
              const hrefPath = new URL(a.getAttribute('href'), location.origin).pathname;
              map[normalizePath(hrefPath)] = cat;
            }
          }
        }
        saveCategoryMap(map);

        const unmatched = books.filter((book) => {
          const cats = bookCategories(book);
          if (!cats.size) return true;
          for (const c of cats) {
            if (availableCategories.has(c)) return false;
          }
          return true;
        });

        const existing = document.getElementById(CATEGORY_ID);
        if (unmatched.length > 0 && !existing) {
          list.appendChild(createFallbackCategoryCard(unmatched.length, isSpanish));
        } else if (unmatched.length === 0 && existing) {
          existing.remove();
        }
        return;
      }

      const pageCategory = detectSubpageCategory(list);
      if (!pageCategory) return;

      const matching = books.filter((book) => {
        if (!matchesLocale(book, pageLocale)) return false;
        return bookCategories(book).has(pageCategory);
      });

      for (const book of matching) {
        if (document.getElementById(`${BOOK_PREFIX}${book.id}`)) continue;
        list.appendChild(createBookCard(book, pageCategory));
      }
    } catch (error) {
      console.error('Could not load imported publications:', error);
    }
  }

  let scheduled = false;
  function schedule() {
    if (scheduled) return;
    scheduled = true;
    setTimeout(() => {
      scheduled = false;
      syncImportedLibrary();
    }, 60);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', schedule, { once: true });
  } else {
    schedule();
  }
  window.addEventListener('pageshow', schedule);
  window.addEventListener('popstate', schedule);

  if (!window.__wolImportedHistoryPatched) {
    window.__wolImportedHistoryPatched = true;
    const origPush = history.pushState;
    const origReplace = history.replaceState;
    history.pushState = function () {
      const r = origPush.apply(this, arguments);
      schedule();
      return r;
    };
    history.replaceState = function () {
      const r = origReplace.apply(this, arguments);
      schedule();
      return r;
    };
  }

  const observer = new MutationObserver((mutations) => {
    if (!isLibraryPage()) return;
    const relevant = mutations.some((m) =>
      [...m.addedNodes].some((n) => n.nodeType === 1 && !(n.id && n.id.startsWith('wol-imported-')))
    );
    if (relevant) schedule();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
})();
