(function () {
  const CATEGORY_ID = 'wol-imported-publications';

  function isLibraryCategoriesPage() {
    if (!/\/wol\/library\/r\d+\/lp-[a-z0-9]+\/[^/]+\/?$/i.test(location.pathname)) return false;
    if (document.querySelector('#materialNav a.backNav, .materialNav a.backNav')) return false;
    return Boolean(document.querySelector('#article ul.directory.navCard, #materialNav ul.directory.navCard, ul.directory.navCard'));
  }

  async function addImportedCategory() {
    if (!isLibraryCategoriesPage()) return;
    const list = document.querySelector('#article ul.directory.navCard, #materialNav ul.directory.navCard, ul.directory.navCard');
    if (!list || document.getElementById(CATEGORY_ID)) return;

    const invoke = window.__TAURI_INTERNALS__?.invoke;
    if (!invoke) return;

    try {
      const books = await invoke('list_imported_publications');
      if (!Array.isArray(books) || !books.length) return;
      if (document.getElementById(CATEGORY_ID)) return;

      const isSpanish = location.pathname.startsWith('/es/') || document.documentElement.lang === 'es';
      const label = isSpanish ? 'Publicaciones importadas' : 'Imported publications';

      const li = document.createElement('li');
      li.id = CATEGORY_ID;
      li.className = 'row card';
      li.setAttribute('role', 'presentation');

      const link = document.createElement('a');
      link.href = `/imported/?from=${encodeURIComponent(location.pathname)}`;
      link.className = 'jwac showRuby chrome cardContainer noTooltips';
      link.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        window.location.assign(link.href);
      });

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
      detail.textContent = String(books.length);

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
      list.appendChild(li);
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
      addImportedCategory();
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

  const observer = new MutationObserver(() => {
    if (isLibraryCategoriesPage() && !document.getElementById(CATEGORY_ID)) {
      schedule();
    }
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
})();
