use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

const WOL_CSS_LINKS: &str = concat!(
    r#"<link rel="stylesheet" type="text/css" href="/assets/css/+0+61168f8ef805085f5a3c0118b14679f74c9e62a4.css">"#,
    r#"<link rel="stylesheet" type="text/css" href="/assets/css/+1+f8319dd08c749624466cb777b7d528e1a42d7368.css">"#,
    r#"<link rel="stylesheet" type="text/css" href="/assets/css/+2+894de4a6eba76bf97927253f95f582449947a96e.css">"#,
    r#"<link rel="stylesheet" type="text/css" href="/assets/css/+3+d00a791fdbb951906e95f1428ae322e41ea9da95.css">"#,
);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subheading {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TocEntry {
    #[serde(default)]
    pub section: Option<String>,
    pub chapter_id: u64,
    #[serde(default)]
    pub kicker: Option<String>,
    pub title: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedChapter {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub chapter_number: Option<u32>,
    #[serde(default)]
    pub subheadings: Vec<Subheading>,
    pub html: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPublication {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub short_title: String,
    pub symbol: String,
    #[serde(default)]
    pub year: u32,
    #[serde(default)]
    pub issue_tag_number: u64,
    pub language: u32,
    pub locale: String,
    pub lib: String,
    pub region: u32,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub publication_type: Option<String>,
    #[serde(default)]
    pub cover_data_url: Option<String>,
    #[serde(default)]
    pub toc: Vec<TocEntry>,
    pub chapters: Vec<ImportedChapter>,
}

fn directory() -> PathBuf {
    crate::dirs_fallback_app_data().join("publications")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn is_spanish(locale: &str) -> bool {
    locale.eq_ignore_ascii_case("es")
}

fn percent_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn query_param(uri: &tauri::http::Uri, key: &str) -> Option<String> {
    let query = uri.query()?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(percent_decode(v));
        }
    }
    None
}

pub fn list() -> Vec<ImportedPublication> {
    let Ok(entries) = fs::read_dir(directory()) else {
        return Vec::new();
    };
    let mut books: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                return None;
            }
            let data = fs::read(entry.path()).ok()?;
            serde_json::from_slice::<ImportedPublication>(&data).ok()
        })
        .collect();
    books.sort_by(|a, b| a.title.cmp(&b.title));
    books
}

#[tauri::command]
pub fn list_imported_publications() -> Vec<serde_json::Value> {
    list()
        .into_iter()
        .map(|book| {
            serde_json::json!({
                "id": book.id,
                "title": book.title,
                "symbol": book.symbol,
                "year": book.year,
                "language": book.language,
                "locale": book.locale,
                "lib": book.lib,
                "region": book.region,
                "category": book.category,
                "categories": book.categories,
                "publicationType": book.publication_type,
                "coverDataUrl": book.cover_data_url,
                "chapterCount": book.chapters.len(),
            })
        })
        .collect()
}

#[tauri::command]
pub fn save_imported_publication(publication: ImportedPublication) -> Result<(), String> {
    if !valid_id(&publication.id) || publication.chapters.is_empty() {
        return Err("Invalid publication".into());
    }
    let data = serde_json::to_vec(&publication).map_err(|e| e.to_string())?;
    if data.len() > 50_000_000 {
        return Err("Publication is too large".into());
    }
    let dir = directory();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.json", publication.id));
    let tmp = dir.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&tmp, data).map_err(|e| e.to_string())?;
    fs::rename(tmp, dest).map_err(|e| e.to_string())
}

pub fn render(uri: &tauri::http::Uri) -> Option<String> {
    let path = uri.path();
    if path == "/imported" || path == "/imported/" {
        let books = list();
        let from = query_param(uri, "from").filter(|s| s.starts_with('/') && !s.starts_with("//"));
        return Some(render_category_page(&books, from.as_deref()));
    }
    let rel = path.strip_prefix("/imported/")?;
    let (book_id, chapter) = rel.split_once('/')?;
    if !valid_id(book_id) {
        return None;
    }
    let book = list().into_iter().find(|b| b.id == book_id)?;
    let from = query_param(uri, "from").filter(|s| s.starts_with('/') && !s.starts_with("//"));
    render_book(&book, chapter, from.as_deref())
}

fn render_header(
    locale: &str,
    lib: &str,
    region: u32,
    pub_nav_html: Option<&str>,
) -> String {
    let lib_upper = lib.to_uppercase();
    let es = is_spanish(locale);
    let home_href = format!("/{}/wol/h/r{}/lp-{}", esc(locale), region, esc(lib));
    let bible_href = format!("/{}/wol/binav/r{}/lp-{}", esc(locale), region, esc(lib));
    let pubs_href = format!(
        "/{}/wol/library/r{}/lp-{}/{}",
        esc(locale),
        region,
        esc(lib),
        if es { "biblioteca" } else { "library" }
    );
    let meetings_href = format!("/{}/wol/meetings/r{}/lp-{}", esc(locale), region, esc(lib));
    let search_action = format!("/{}/wol/qt/r{}/lp-{}", esc(locale), region, esc(lib));

    let site_title = if es {
        "BIBLIOTECA EN LÍNEA Watchtower"
    } else {
        "Watchtower ONLINE LIBRARY"
    };
    let site_line2 = if es {
        "BIBLIOTECA EN LÍNEA"
    } else {
        "ONLINE LIBRARY"
    };
    let bible_label = if es { "BIBLIA" } else { "BIBLE" };
    let pubs_label = if es { "PUBLICACIONES" } else { "PUBLICATIONS" };
    let meetings_label = if es { "REUNIONES" } else { "MEETINGS" };
    let search_placeholder = if es { "Introducir búsqueda" } else { "Search" };
    let search_btn = if es { "BUSCAR" } else { "SEARCH" };
    let nav_extra = pub_nav_html.unwrap_or("");

    format!(
        r#"<div id="regionHeader" class="chrome showRuby ml-{lib_upper} ms-ROMAN dir-ltr" lang="{locale}" data-lang="{lib_upper}" dir="ltr">
  <div id="siteBanner">
    <div class="title">
      <a href="{home_href}">
        <span class="logo"><span class="icon"></span></span>
        <div class="ellipsized label">
          <span class="ellipsized titleBig">{site_title}</span>
          <div class="ellipsized titleLittle siteTitleLine1">Watchtower</div>
          <div class="ellipsized titleLittle siteTitleLine2">{site_line2}</div>
        </div>
      </a>
    </div>
  </div>
  <div id="toolbarFrame">
    <div id="standardSearch" class="searchControl">
      <form class="searchForm" role="search" method="GET" action="{search_action}">
        <div class="searchFieldContainer">
          <input class="searchField" name="q" accept-charset="utf-8" type="text" placeholder="{search_placeholder}" autocomplete="off" spellcheck="false" maxlength="256" value="">
        </div>
        <div class="searchControlContainer">
          <input type="hidden" name="p" value="par">
          <input type="hidden" name="r" value="occ">
          <input class="searchButton" type="submit" value="{search_btn}">
        </div>
      </form>
    </div>
    <div id="toolbarMenu">
      <ul class="menu" role="menu">
        <li id="menuBible" class="menuButton chrome showRuby ml-{lib_upper} ms-ROMAN dir-ltr" role="menuitem"><a class="bibleNav" href="{bible_href}"><span class="icon"></span><span class="label">{bible_label}</span></a></li>
        <li id="menuPublications" class="menuButton chrome showRuby ml-{lib_upper} ms-ROMAN dir-ltr" role="menuitem"><a class="libraryNav" href="{pubs_href}"><span class="icon"></span><span class="label">{pubs_label}</span></a></li>
        <li id="menuToday" class="menuButton chrome showRuby ml-{lib_upper} ms-ROMAN dir-ltr" role="menuitem"><a class="todayNav" href="{meetings_href}"><span class="icon"></span><span class="label">{meetings_label}</span></a></li>
      </ul>
    </div>
  </div>
  {nav_extra}
</div>"#,
        lib_upper = esc(&lib_upper),
        locale = esc(locale),
    )
}

fn render_shell(locale: &str, title: &str, header_html: &str, main_html: &str) -> String {
    let fallback_styles = r#"
    <style>
      .wol-imported-empty { padding: 2em 1.2em; color: #555; }
      #publicationNavigation .resultNavControls .disabled { opacity: 0.35; pointer-events: none; }
      #subheadings .documentNavigation li.subheading a { display: block; }
      #regionMain.showSecondaryNav #secondaryNavContent { background-color: #f2f1ef; bottom: 0; }
    </style>"#;
    format!(
        r#"<!doctype html>
<html lang="{locale}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>{title}</title>
  {WOL_CSS_LINKS}
  {fallback_styles}
</head>
<body class="showRuby cc-theme--light DynamicLayout">
  <div id="wrapper" class="dropShadow50">
    {header_html}
    {main_html}
  </div>
</body>
</html>"#,
        locale = esc(locale),
        title = esc(title),
    )
}

fn render_category_page(books: &[ImportedPublication], back_override: Option<&str>) -> String {
    let (locale, lib, region) = books
        .first()
        .map(|b| (b.locale.as_str(), b.lib.as_str(), b.region))
        .unwrap_or(("es", "s", 4));
    let es = is_spanish(locale);
    let lib_upper = lib.to_uppercase();
    let heading = if es {
        "Publicaciones importadas"
    } else {
        "Imported publications"
    };
    let default_back = format!(
        "/{}/wol/library/r{}/lp-{}/{}",
        esc(locale),
        region,
        esc(lib),
        if es { "biblioteca" } else { "library" }
    );
    let back_href = back_override.map(esc).unwrap_or(default_back);

    let items_html: String = if books.is_empty() {
        format!(
            r#"<p class="wol-imported-empty">{}</p>"#,
            if es {
                "No hay publicaciones importadas todavía. Usa Configuración → Load JWPUB para importar una."
            } else {
                "No imported publications yet. Use Settings → Load JWPUB to import one."
            }
        )
    } else {
        let rows: String = books
            .iter()
            .map(|book| {
                let thumb = match &book.cover_data_url {
                    Some(url) => format!(
                        r#"<img class="cardThumbnailImage thumbnail document icon-default publication" src="{}" alt="">"#,
                        esc(url)
                    ),
                    None => r#"<span class="cardThumbnailImage thumbnail document jwf-jw-icons-all jwi-book-stack icon-default"></span>"#.to_string(),
                };
                let year = if book.year > 0 {
                    book.year.to_string()
                } else {
                    String::new()
                };
                format!(
                    r#"<li class="row card" role="presentation">
  <a href="/imported/{id}/index" class="jwac showRuby chrome cardContainer noTooltips lnk">
    <div class="cardThumbnail">{thumb}</div>
    <div class="cardTitleBlock">
      <div class="cardLine1 ellipsized cardSingleLine"><span class="sectionIcon"></span>{title} ({symbol})</div>
      <div class="hidden"></div>
    </div>
    <div class="cardTitleDetail">{year}</div>
    <div class="cardChevron"><div class="icon"></div></div>
  </a>
</li>"#,
                    id = esc(&book.id),
                    title = esc(&book.title),
                    symbol = esc(&book.symbol),
                )
            })
            .collect();
        format!(r#"<ul class="directory navCard" role="directory">{rows}</ul>"#)
    };

    let header = render_header(locale, lib, region, None);
    let main = format!(
        r#"<div id="regionMain" class="unified showRuby ml-{lib_upper} ms-ROMAN dir-ltr" lang="{locale}" data-lang="{lib_upper}" dir="ltr">
  <div id="content" class="content">
    <article id="article" class="article libraryNav html5 jwac showRuby ml-{lib_upper} ms-ROMAN dir-ltr contentOnly" lang="{locale}" data-lang="{lib_upper}" dir="ltr" role="article">
      <div id="materialNav" role="navigation">
        <nav>
          <h1><a class="backNav" href="{back_href}"><span class="icon"></span><span class="title">{heading}</span></a></h1>
          {items_html}
        </nav>
      </div>
    </article>
  </div>
</div>"#,
        lib_upper = esc(&lib_upper),
        locale = esc(locale),
    );
    render_shell(locale, heading, &header, &main)
}

fn render_toc_row(book: &ImportedPublication, entry: &TocEntry, is_first: bool) -> String {
    let thumb = if is_first {
        match &book.cover_data_url {
            Some(url) => format!(
                r#"<img class="cardThumbnailImage thumbnail document icon-default" src="{}" alt="">"#,
                esc(url)
            ),
            None => r#"<span class="cardThumbnailImage thumbnail document icon-default"></span>"#.to_string(),
        }
    } else {
        r#"<span class="cardThumbnailImage thumbnail document icon-default"></span>"#.to_string()
    };

    let title_block = match &entry.kicker {
        Some(kicker) if !kicker.is_empty() => format!(
            r#"<div class="cardTitleBlock">
  <div class="cardLine1 ellipsized cardHalfHeight cardLine2Prominent"><span class="sectionIcon"></span>{}</div>
  <div class="cardLine2 ellipsized cardLine2Prominent">{}</div>
</div>"#,
            esc(kicker),
            esc(&entry.title),
        ),
        _ => format!(
            r#"<div class="cardTitleBlock">
  <div class="cardLine1 ellipsized cardSingleLine"><span class="sectionIcon"></span>{}</div>
  <div class="hidden"></div>
</div>"#,
            esc(&entry.title),
        ),
    };

    let link_class = if entry.kicker.is_some() {
        "jwac showRuby chrome cardContainer noTooltips cardLine2Prominent lnk"
    } else {
        "jwac showRuby chrome cardContainer noTooltips lnk"
    };

    format!(
        r#"<li class="row card navCard" role="presentation">
  <a href="/imported/{id}/{ch_id}" class="{link_class}">
    <div class="cardThumbnail">{thumb}</div>
    {title_block}
    <div class="cardTitleDetail"></div>
    <div class="cardChevron"><div class="icon"></div></div>
  </a>
</li>"#,
        id = esc(&book.id),
        ch_id = entry.chapter_id,
    )
}

fn render_index_page(book: &ImportedPublication, back_override: Option<&str>) -> String {
    let lib_upper = book.lib.to_uppercase();
    let entries: Vec<TocEntry> = if book.toc.is_empty() {
        book.chapters
            .iter()
            .map(|c| TocEntry {
                section: None,
                chapter_id: c.id,
                kicker: c.chapter_number.map(|n| format!("CAPÍTULO {n}")),
                title: c.title.clone(),
            })
            .collect()
    } else {
        book.toc.clone()
    };

    let mut directory_html = String::new();
    let mut current_section: Option<String> = None;

    for (idx, entry) in entries.iter().enumerate() {
        if entry.section != current_section {
            if current_section.is_some() {
                directory_html.push_str("</ul></li>");
            }
            current_section = entry.section.clone();
            if let Some(sec) = &current_section {
                directory_html.push_str(&format!(
                    r#"<li class="group"><div class="title">{}</div><ul>"#,
                    esc(sec)
                ));
            }
        }
        directory_html.push_str(&render_toc_row(book, entry, idx == 0));
    }
    if current_section.is_some() {
        directory_html.push_str("</ul></li>");
    }

    let back_href = back_override.map(esc).unwrap_or_else(|| "/imported/".to_string());
    let header = render_header(&book.locale, &book.lib, book.region, None);
    let main = format!(
        r#"<div id="regionMain" class="unified showRuby ml-{lib_upper} ms-ROMAN dir-ltr" lang="{locale}" data-lang="{lib_upper}" dir="ltr">
  <div id="content" class="content">
    <article id="article" class="article publicationNav html5 jwac showRuby ml-{lib_upper} ms-ROMAN dir-ltr contentOnly" lang="{locale}" data-lang="{lib_upper}" dir="ltr" role="article">
      <div class="materialNav" role="navigation">
        <nav>
          <header>
            <h1><a class="backNav" href="{back_href}"><span class="icon"></span>{title}</a></h1>
          </header>
          <ul class="directory thumbnails" role="directory">{directory_html}</ul>
        </nav>
      </div>
    </article>
  </div>
</div>"#,
        lib_upper = esc(&lib_upper),
        locale = esc(&book.locale),
        title = esc(&book.title),
    );
    render_shell(&book.locale, &book.title, &header, &main)
}

fn render_chapter_page(book: &ImportedPublication, chapter_id: u64) -> Option<String> {
    let pos = book.chapters.iter().position(|c| c.id == chapter_id)?;
    let ch = &book.chapters[pos];
    let prev = pos.checked_sub(1).and_then(|i| book.chapters.get(i));
    let next = book.chapters.get(pos + 1);
    let lib_upper = book.lib.to_uppercase();
    let es = is_spanish(&book.locale);

    let short_sym = if book.short_title.is_empty() {
        &book.symbol
    } else {
        &book.symbol
    };
    let breadcrumb_label = match ch.chapter_number {
        Some(n) => format!("{} cap. {}", short_sym, n),
        None => format!("{} — {}", short_sym, ch.title),
    };

    let prev_html = match prev {
        Some(p) => format!(
            r#"<li class="resultNavLeft"><a class="resultsNavDocumentLink" href="/imported/{}/{}" title="{}"><span class="icon"></span></a></li>"#,
            esc(&book.id),
            p.id,
            esc(&p.title)
        ),
        None => r#"<li class="resultNavLeft"><span class="resultsNavDocumentLink disabled"><span class="icon"></span></span></li>"#.to_string(),
    };
    let next_html = match next {
        Some(n) => format!(
            r#"<li class="resultNavRight"><a class="resultsNavDocumentLink" href="/imported/{}/{}" title="{}"><span class="icon"></span></a></li>"#,
            esc(&book.id),
            n.id,
            esc(&n.title)
        ),
        None => r#"<li class="resultNavRight"><span class="resultsNavDocumentLink disabled"><span class="icon"></span></span></li>"#.to_string(),
    };

    let pub_nav_html = format!(
        r#"<div id="publicationNavigation" class="chrome">
  <div class="secondaryNavToggle" title="{toggle_title}"><span class="icon"></span></div>
  <div class="pubNavTitle">
    <div class="chrome showRuby ml-{lib_upper} ms-ROMAN dir-ltr" lang="{locale}" data-lang="{lib_upper}" dir="ltr">
      <ul>
        <li class="resultsNavigationSelected documentLocation navPublications"><a href="/imported/{book_id}/index" class="spriteLink ellipsized"><span class="icon"></span>{breadcrumb_label}</a></li>
        <li class="resultDocumentPubTitle ellipsized">{ch_title}</li>
      </ul>
    </div>
  </div>
  <div class="chrome forwardBackNavControls resultNavControls">
    <ul>{prev_html}{next_html}</ul>
  </div>
</div>"#,
        toggle_title = if es { "Mostrar u ocultar barra lateral" } else { "Toggle sidebar" },
        lib_upper = esc(&lib_upper),
        locale = esc(&book.locale),
        book_id = esc(&book.id),
        breadcrumb_label = esc(&breadcrumb_label),
        ch_title = esc(&ch.title),
    );

    let subheadings_list: String = ch
        .subheadings
        .iter()
        .map(|sh| {
            format!(
                r##"<li class="subheading"><a href="#{}">{}</a></li>"##,
                esc(&sh.id),
                esc(&sh.title)
            )
        })
        .collect();

    let subheadings_tab_label = if es { "Subtítulos" } else { "Subheadings" };
    let issue_input = if book.issue_tag_number > 0 {
        format!(
            r#"<input type="hidden" id="issue" value="{}">"#,
            book.issue_tag_number
        )
    } else {
        String::new()
    };

    let secondary_nav_classes = format!(
        "unified navLeft showRuby ml-{lib_upper} ms-ROMAN dir-ltr hasSecondaryNav",
        lib_upper = esc(&lib_upper)
    );

    let sidebar_script = r##"<script>
(function () {
  const regionMain = document.getElementById('regionMain');
  const toggle = document.querySelector('#publicationNavigation .secondaryNavToggle');
  const navList = document.querySelector('#subheadings ul.documentNavigation');
  if (navList && navList.children.length === 0) {
    const article = document.getElementById('article');
    if (article) {
      let headings = Array.from(article.querySelectorAll('h2[id][data-pid]'));
      if (headings.length === 0) headings = Array.from(article.querySelectorAll('h3[id][data-pid]'));
      for (const h of headings) {
        const text = (h.textContent || '').replace(/\s+/g, ' ').trim();
        if (!h.id || !text) continue;
        const li = document.createElement('li');
        li.className = 'subheading';
        const a = document.createElement('a');
        a.href = '#' + h.id;
        a.textContent = text;
        li.appendChild(a);
        navList.appendChild(li);
      }
    }
  }
  const hasSubheadings = Boolean(navList && navList.children.length > 0);
  if (regionMain && hasSubheadings && window.innerWidth >= 960) {
    regionMain.classList.add('showSecondaryNav');
  }
  if (toggle && regionMain) {
    toggle.addEventListener('click', function () {
      regionMain.classList.toggle('showSecondaryNav');
    });
  }
  document.addEventListener('click', function (e) {
    const link = e.target && e.target.closest ? e.target.closest('a[href^="#"]') : null;
    if (!link) return;
    const id = link.getAttribute('href').slice(1);
    const target = id ? document.getElementById(id) : null;
    if (target) {
      e.preventDefault();
      target.scrollIntoView({ behavior: 'smooth', block: 'start' });
      history.replaceState(null, '', '#' + id);
      if (regionMain && window.innerWidth < 960) {
        regionMain.classList.remove('showSecondaryNav');
      }
    }
  });
})();
</script>"##;

    let header = render_header(&book.locale, &book.lib, book.region, Some(&pub_nav_html));
    let main = format!(
        r#"<div id="regionMain" class="{secondary_nav_classes}" lang="{locale}" data-lang="{lib_upper}" dir="ltr">
  <div id="secondaryNavContent" class="chrome showRuby ml-{lib_upper} ms-ROMAN dir-ltr" lang="{locale}" data-lang="{lib_upper}" dir="ltr">
    <div id="secondaryNav" role="navigation">
      <nav>
        <div id="resultNavigation">
          <div class="resultNavTitle">
            <ul>
              <li class="resultsNavigationSelected"><span><span class="navContent">{ch_title}</span></span></li>
              <li class="resultDocumentPubTitle"><span><span class="navContent">{book_title}</span></span></li>
            </ul>
          </div>
        </div>
      </nav>
      <div id="documentNavigation" class="chrome">
        <ul class="navigationTabs">
          <li class="active tabItem" data-related-content="subheadings">
            <div class="chrome showRuby ml-{lib_upper} ms-ROMAN dir-ltr">{subheadings_tab_label}</div>
          </li>
        </ul>
        <div class="navigationContents">
          <div id="subheadings" class="active tabContent" role="navigation">
            <nav>
              <ul class="documentNavigation noTooltips" role="navigation">{subheadings_list}</ul>
            </nav>
          </div>
        </div>
      </div>
    </div>
  </div>
  <div id="content" class="content">
    <div class="articlePositioner" lang="{locale}" data-lang="{lib_upper}" dir="ltr">
      <div id="banner" class="documentBanner showRuby ml-{lib_upper} ms-ROMAN dir-ltr" lang="{locale}" data-lang="{lib_upper}" dir="ltr"></div>
      {issue_input}
      <article id="article" class="article docId-{ch_id} pub-{symbol} html5 jwac showRuby ml-{lib_upper} ms-ROMAN dir-ltr" lang="{locale}" data-lang="{lib}" data-meps-lang="{meps_lang}" dir="ltr" role="article">
        {ch_html}
      </article>
    </div>
  </div>
</div>
{sidebar_script}"#,
        lib_upper = esc(&lib_upper),
        locale = esc(&book.locale),
        lib = esc(&book.lib),
        meps_lang = book.language,
        ch_title = esc(&ch.title),
        book_title = esc(&book.title),
        ch_id = ch.id,
        symbol = esc(&book.symbol),
        ch_html = ch.html,
    );

    let page_title = format!("{} — {}", ch.title, book.title);
    Some(render_shell(&book.locale, &page_title, &header, &main))
}

fn render_book(book: &ImportedPublication, chapter: &str, back_override: Option<&str>) -> Option<String> {
    if chapter == "index" {
        Some(render_index_page(book, back_override))
    } else {
        let id: u64 = chapter.parse().ok()?;
        render_chapter_page(book, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_book() -> ImportedPublication {
        ImportedPublication {
            id: "1-sfg-2025-0".into(),
            title: "Pastoreen el rebaño de Dios".into(),
            short_title: "Pastoreen".into(),
            symbol: "sfg".into(),
            year: 2025,
            issue_tag_number: 0,
            language: 1,
            locale: "es".into(),
            lib: "s".into(),
            region: 4,
            category: Some("manual".into()),
            categories: vec!["manual".into()],
            publication_type: Some("Manual/Guidelines".into()),
            cover_data_url: Some("data:image/jpeg;base64,AAAA".into()),
            toc: vec![
                TocEntry {
                    section: None,
                    chapter_id: 1102025558,
                    kicker: None,
                    title: "Portada".into(),
                },
                TocEntry {
                    section: Some("CAPÍTULOS".into()),
                    chapter_id: 1102025561,
                    kicker: Some("CAPÍTULO 1".into()),
                    title: "Los ancianos colaboran como un cuerpo".into(),
                },
            ],
            chapters: vec![
                ImportedChapter {
                    id: 1102025558,
                    title: "Portada".into(),
                    chapter_number: None,
                    subheadings: vec![],
                    html: "<p data-pid=\"1\">Portada</p>".into(),
                },
                ImportedChapter {
                    id: 1102025561,
                    title: "Los ancianos colaboran como un cuerpo".into(),
                    chapter_number: Some(1),
                    subheadings: vec![Subheading {
                        id: "p29".into(),
                        title: "REUNIONES DE ANCIANOS".into(),
                    }],
                    html: "<p data-pid=\"24\">Text</p>".into(),
                },
            ],
        }
    }

    #[test]
    fn renders_article_with_stable_document_and_language_identifiers() {
        let book = sample_book();
        let index = render_book(&book, "index", Some("/es/wol/library/r4/lp-s/biblioteca/pautas-y-manuales")).unwrap();
        assert!(index.contains("/imported/1-sfg-2025-0/1102025561"));
        assert!(index.contains(r#"href="/es/wol/library/r4/lp-s/biblioteca/pautas-y-manuales""#));
        assert!(index.contains("publicationNav"));
        assert!(index.contains(r#"<li class="group"><div class="title">CAPÍTULOS</div>"#));
        assert!(index.contains("CAPÍTULO 1"));

        let page = render_book(&book, "1102025561", None).unwrap();
        assert!(page.contains("docId-1102025561 pub-sfg"));
        assert!(page.contains("data-lang=\"s\""));
        assert!(page.contains("data-meps-lang=\"1\""));
        assert!(page.contains("data-pid=\"24\""));
        assert!(page.contains("REUNIONES DE ANCIANOS"));
        assert!(page.contains("/imported/1-sfg-2025-0/1102025558"));
        assert!(render_book(&book, "999", None).is_none());
    }

    #[test]
    fn renders_imported_category_page() {
        let book = sample_book();
        let html = render_category_page(&[book], Some("/es/wol/library/r4/lp-s/biblioteca"));
        assert!(html.contains("Publicaciones importadas"));
        assert!(html.contains("/imported/1-sfg-2025-0/index"));
        assert!(html.contains("Pastoreen el rebaño de Dios (sfg)"));
        assert!(html.contains("2025"));
    }
}
