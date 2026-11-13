use regex::Regex;
use std::sync::OnceLock;

fn html_ext_subresource_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(<(?:link|script|img)\b[^>]*?\b(?:href|src)=["'])https?://([^/"']+)/"#)
            .expect("static html subresource regex must compile")
    })
}

fn css_ext_url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"url\((["']?)https?://([^/"']+)/"#)
            .expect("static css url regex must compile")
    })
}

/// Rewrites HTML and CSS content to route root-relative subresources and external CDN requests
/// through the `wol://` custom scheme.
pub fn rewrite_html(html: &str, host: &str) -> String {
    // External CDN subresources in <link>, <script>, <img>
    let cdn_repl = format!("${{1}}wol://{host}/__cdn__/${{2}}/");
    let mut result = html_ext_subresource_regex()
        .replace_all(html, cdn_repl.as_str())
        .into_owned();

    // Root-relative asset paths in attributes (href, src)
    let asset_target = format!("wol://{host}/assets/");
    result = result.replace("href=\"/assets/", &format!("href=\"{asset_target}"));
    result = result.replace("src=\"/assets/", &format!("src=\"{asset_target}"));
    result = result.replace("href='/assets/", &format!("href='{asset_target}"));
    result = result.replace("src='/assets/", &format!("src='{asset_target}"));

    // Root-relative icon paths
    let icon_target = format!("wol://{host}/icon/");
    result = result.replace("href=\"/icon/", &format!("href=\"{icon_target}"));
    result = result.replace("src=\"/icon/", &format!("src=\"{icon_target}"));
    result = result.replace("href='/icon/", &format!("href='{icon_target}"));
    result = result.replace("src='/icon/", &format!("src='{icon_target}"));

    // Root-relative img paths
    let img_target = format!("wol://{host}/img/");
    result = result.replace("href=\"/img/", &format!("href=\"{img_target}"));
    result = result.replace("src=\"/img/", &format!("src=\"{img_target}"));
    result = result.replace("href='/img/", &format!("href='{img_target}"));
    result = result.replace("src='/img/", &format!("src='{img_target}"));

    // CSS url() references if any are inlined
    result = result.replace("url(/assets/", &format!("url({asset_target}"));
    result = result.replace("url(\"/assets/", &format!("url(\"{asset_target}"));
    result = result.replace("url('/assets/", &format!("url('{asset_target}"));
    result = result.replace("url(/img/", &format!("url({img_target}"));
    result = result.replace("url(\"/img/", &format!("url(\"{img_target}"));
    result = result.replace("url('/img/", &format!("url('{img_target}"));

    result
}

/// Rewrites CSS stylesheets if they contain absolute or CDN paths
pub fn rewrite_css(css: &str, host: &str) -> String {
    let cdn_repl = format!("url(${{1}}wol://{host}/__cdn__/${{2}}/");
    let mut result = css_ext_url_regex()
        .replace_all(css, cdn_repl.as_str())
        .into_owned();

    let asset_target = format!("wol://{host}/assets/");
    result = result.replace("url(/assets/", &format!("url({asset_target}"));
    result = result.replace("url(\"/assets/", &format!("url(\"{asset_target}"));
    result = result.replace("url('/assets/", &format!("url('{asset_target}"));

    let img_target = format!("wol://{host}/img/");
    result = result.replace("url(/img/", &format!("url({img_target}"));
    result = result.replace("url(\"/img/", &format!("url(\"{img_target}"));
    result = result.replace("url('/img/", &format!("url('{img_target}"));

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_assets_in_html() {
        let input = r#"<link rel="stylesheet" href="/assets/css/style.css"><script src="/assets/js/app.js"></script>"#;
        let rewritten = rewrite_html(input, "es");
        assert_eq!(
            rewritten,
            r#"<link rel="stylesheet" href="wol://es/assets/css/style.css"><script src="wol://es/assets/js/app.js"></script>"#
        );
    }

    #[test]
    fn rewrites_icons_in_html() {
        let input = r#"<link rel="icon" href="/icon/favicon-32x32.png">"#;
        let rewritten = rewrite_html(input, "es");
        assert_eq!(
            rewritten,
            r#"<link rel="icon" href="wol://es/icon/favicon-32x32.png">"#
        );
    }

    #[test]
    fn rewrites_cdn_in_html() {
        let input = r#"<link rel="stylesheet" href="https://cdn.example.org/code/media-player/v3.3.0/css/media-player.css"/>"#;
        let rewritten = rewrite_html(input, "es");
        assert_eq!(
            rewritten,
            r#"<link rel="stylesheet" href="wol://es/__cdn__/cdn.example.org/code/media-player/v3.3.0/css/media-player.css"/>"#
        );
    }

    #[test]
    fn rewrites_css_urls() {
        let input = r#"@font-face { src: url(/assets/fonts/font.woff2); } .logo { background: url('/assets/img/logo.png'); }"#;
        let rewritten = rewrite_css(input, "es");
        assert_eq!(
            rewritten,
            r#"@font-face { src: url(wol://es/assets/fonts/font.woff2); } .logo { background: url('wol://es/assets/img/logo.png'); }"#
        );
    }
}
