//! Trust engine facade and rendering orchestration.

use fortrust_core::{DecodedImage, ImageRegistry, PrivacyConfig, PrivacyEngine, RequestContext, ResourceType};
use fortrust_dom::{DomArena, NodeRef, parse_html};
use fortrust_net::{FetchSource, NetprocClient, NetworkClient, NetworkError};
use fortrust_renderer::{RenderError, RenderedPage, StaticRenderer};
use futures_util::StreamExt;
use url::Url;

pub use fortrust_layout::Rect as EngineRect;
pub use fortrust_paint::{DisplayCommand, DisplayList};
pub use fortrust_renderer::Viewport;
pub use fortrust_style::{BorderStyle, Color, OutlineStyle};

pub const TRUST_ENGINE_NAME: &str = "Trust Engine";
pub const TRUST_ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_EXTERNAL_STYLESHEETS_PER_DOCUMENT: usize = 12;
const MAX_EXTERNAL_STYLESHEET_BYTES_PER_DOCUMENT: usize = 1024 * 1024;
const MAX_EXTERNAL_IMAGES_PER_DOCUMENT: usize = 32;
const MAX_EXTERNAL_IMAGE_BYTES_PER_DOCUMENT: usize = 4 * 1024 * 1024;
const MAX_EXTERNAL_SUBRESOURCE_BYTES_PER_DOCUMENT: usize = 6 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMode {
    Offline,
    Networked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSource {
    Internal,
    Offline,
    Network,
    Cache,
    RevalidatedCache,
}

impl From<FetchSource> for PageSource {
    fn from(source: FetchSource) -> Self {
        match source {
            FetchSource::Cache => Self::Cache,
            FetchSource::Network => Self::Network,
            FetchSource::RevalidatedCache => Self::RevalidatedCache,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityReport {
    pub privacy_pipeline_enforced: bool,
    pub javascript_enabled: bool,
    pub external_subresources_enabled: bool,
    pub sandboxed_static_render: bool,
    pub source: PageSource,
    pub body_bytes: usize,
    pub display_commands: usize,
    pub parse_error_count: usize,
    pub external_stylesheets_loaded: usize,
    pub external_stylesheets_blocked: usize,
    pub external_images_loaded: usize,
    pub external_images_blocked: usize,
    pub cosmetic_stylesheets_injected: usize,
}

impl SecurityReport {
    #[allow(clippy::too_many_arguments)]
    fn for_render(
        source: PageSource,
        body_bytes: usize,
        rendered: &RenderedPage,
        javascript_enabled: bool,
        external_stylesheets_loaded: usize,
        external_stylesheets_blocked: usize,
        external_images_loaded: usize,
        external_images_blocked: usize,
    ) -> Self {
        Self {
            privacy_pipeline_enforced: true,
            javascript_enabled,
            external_subresources_enabled: external_stylesheets_loaded > 0
                || external_stylesheets_blocked > 0
                || external_images_loaded > 0
                || external_images_blocked > 0,
            sandboxed_static_render: true,
            source,
            body_bytes,
            display_commands: rendered.display_list.len(),
            parse_error_count: rendered.parse_error_count,
            external_stylesheets_loaded,
            external_stylesheets_blocked,
            external_images_loaded,
            external_images_blocked,
            cosmetic_stylesheets_injected: rendered.injected_css.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnginePage {
    pub url: String,
    pub title: String,
    pub rendered: RenderedPage,
    pub security: SecurityReport,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    NetworkUnavailable,
    Network(NetworkError),
    Render(RenderError),
    Dom(fortrust_dom::DomError),
    Js(String),
    InvalidUtf8Document,
}

impl From<RenderError> for EngineError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<NetworkError> for EngineError {
    fn from(error: NetworkError) -> Self {
        Self::Network(error)
    }
}

impl From<fortrust_dom::DomError> for EngineError {
    fn from(error: fortrust_dom::DomError) -> Self {
        Self::Dom(error)
    }
}

#[cfg(feature = "javascript")]
impl From<fortrust_js::JsError> for EngineError {
    fn from(error: fortrust_js::JsError) -> Self {
        Self::Js(error.to_string())
    }
}

pub struct TrustEngine {
    renderer: StaticRenderer,
    network: Option<NetworkClient>,
    netproc: Option<NetprocClient>,
    mode: EngineMode,
    // runtime flags to control optional capabilities
    pub javascript_enabled: bool,
    pub allow_external_subresources: bool,
}

impl TrustEngine {
    pub fn offline() -> Self {
        Self {
            renderer: StaticRenderer::new(),
            network: None,
            netproc: None,
            mode: EngineMode::Offline,
            javascript_enabled: cfg!(feature = "javascript"),
            allow_external_subresources: true,
        }
    }

    pub fn secure_networked(privacy: PrivacyConfig) -> Result<Self, EngineError> {
        let network = NetworkClient::new(PrivacyEngine::new(privacy))?;
        Ok(Self {
            renderer: StaticRenderer::new(),
            network: Some(network),
            netproc: None,
            mode: EngineMode::Networked,
            javascript_enabled: cfg!(feature = "javascript"),
            allow_external_subresources: true,
        })
    }

    /// Create a TrustEngine that routes the main document fetch through an external
    /// `netproc` process. Subresource loading falls back to the in-process `NetworkClient`.
    pub fn with_netproc(
        privacy: PrivacyConfig,
        netproc: NetprocClient,
    ) -> Result<Self, EngineError> {
        let network = NetworkClient::new(PrivacyEngine::new(privacy))?;
        Ok(Self {
            renderer: StaticRenderer::new(),
            network: Some(network),
            netproc: Some(netproc),
            mode: EngineMode::Networked,
            javascript_enabled: cfg!(feature = "javascript"),
            allow_external_subresources: true,
        })
    }

    /// Async constructor: connects to netproc at the given address, then creates the engine.
    pub async fn with_netproc_async(
        privacy: PrivacyConfig,
        netproc_addr: &str,
    ) -> Result<Self, EngineError> {
        let netproc = NetprocClient::connect(netproc_addr)
            .await
            .map_err(|e| EngineError::Network(NetworkError::Transport(format!("netproc: {e:?}"))))?;
        Self::with_netproc(privacy, netproc)
    }

    pub fn mode(&self) -> EngineMode {
        self.mode
    }

    pub fn render_html(
        &self,
        url: impl Into<String>,
        html: &str,
        author_css: &[&str],
        viewport: Viewport,
    ) -> Result<EnginePage, EngineError> {
        let url = url.into();
        self.build_page(
            url,
            html,
            author_css,
            &[],
            viewport,
            PageSource::Offline,
            0,
            0,
            0,
            0,
            Vec::new(),
        )
    }

    pub fn render_html_with_cosmetic(
        &self,
        url: impl Into<String>,
        html: &str,
        author_css: &[&str],
        cosmetic_css: &[&str],
        viewport: Viewport,
    ) -> Result<EnginePage, EngineError> {
        let url = url.into();
        self.build_page(
            url,
            html,
            author_css,
            cosmetic_css,
            viewport,
            PageSource::Offline,
            0,
            0,
            0,
            0,
            Vec::new(),
        )
    }

    pub fn internal_page(
        &self,
        url: impl Into<String>,
        viewport: Viewport,
    ) -> Result<EnginePage, EngineError> {
        let url = url.into();
        let html = internal_html(&url);
        self.build_page(url, &html, &[], &[], viewport, PageSource::Internal, 0, 0, 0, 0, Vec::new())
    }

    pub async fn load_url(
        &mut self,
        url: impl Into<String>,
        viewport: Viewport,
    ) -> Result<EnginePage, EngineError> {
        self.load_url_with_cosmetic(url, viewport, &[]).await
    }

    pub async fn load_url_with_cosmetic(
        &mut self,
        url: impl Into<String>,
        viewport: Viewport,
        cosmetic_css: &[&str],
    ) -> Result<EnginePage, EngineError> {
        let url = url.into();

        // Main document fetch: prefer netproc, fall back to in-process NetworkClient
        let response = if let Some(ref netproc) = self.netproc {
            netproc.fetch(RequestContext::document(url.clone())).await?
        } else if let Some(ref mut network) = self.network {
            network.fetch(RequestContext::document(url.clone())).await?
        } else {
            return Err(EngineError::NetworkUnavailable);
        };

        let html =
            std::str::from_utf8(&response.body).map_err(|_| EngineError::InvalidUtf8Document)?;
        let page_url = response.url.to_string();

        // Subresource loading (stylesheets, images) always uses in-process NetworkClient
        let (author_css, external_stylesheets_blocked, external_images_loaded, external_images_blocked, decoded_images) = if self.allow_external_subresources {
            if let Some(ref mut network) = self.network {
                let (author_css, external_stylesheets_blocked) =
                    load_external_stylesheets(network, &page_url, html).await?;
                let (external_images_loaded, external_images_blocked, decoded_images) =
                    load_external_images(network, &page_url, html).await?;
                (author_css, external_stylesheets_blocked, external_images_loaded, external_images_blocked, decoded_images)
            } else {
                (Vec::new(), 0usize, 0usize, 0usize, Vec::new())
            }
        } else {
            (Vec::new(), 0usize, 0usize, 0usize, Vec::new())
        };
        let author_css_refs = author_css.iter().map(String::as_str).collect::<Vec<_>>();
        self.build_page(
            page_url,
            html,
            &author_css_refs,
            cosmetic_css,
            viewport,
            response.source.into(),
            author_css.len(),
            external_stylesheets_blocked,
            external_images_loaded,
            external_images_blocked,
            decoded_images,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_page(
        &self,
        url: String,
        html: &str,
        author_css: &[&str],
        cosmetic_css: &[&str],
        viewport: Viewport,
        source: PageSource,
        external_stylesheets_loaded: usize,
        external_stylesheets_blocked: usize,
        external_images_loaded: usize,
        external_images_blocked: usize,
        decoded_images: Vec<DecodedImage>,
    ) -> Result<EnginePage, EngineError> {
        let javascript_enabled = self.javascript_enabled && cfg!(feature = "javascript");
        let mut images = fortrust_core::ImageRegistry::new();
        for img in decoded_images {
            images.insert(img);
        }
        let (rendered, js_title_opt) = if javascript_enabled {
                render_with_javascript(html, author_css, cosmetic_css, viewport, &url, images)?
            } else {
                let all_css = [author_css, cosmetic_css].concat();
                (self.renderer.render_with_images(html, &all_css, viewport, images)?, None)
            };
        let security = SecurityReport::for_render(
            source,
            html.len(),
            &rendered,
            javascript_enabled,
            external_stylesheets_loaded,
            external_stylesheets_blocked,
            external_images_loaded,
            external_images_blocked,
        );
        let title = js_title_opt
            .unwrap_or_else(|| title_from_html_or_url(html, &url));

        Ok(EnginePage {
            title,
            url,
            rendered,
            security,
        })
    }
}

#[cfg(feature = "javascript")]
fn render_with_javascript(
    html: &str,
    author_css: &[&str],
    cosmetic_css: &[&str],
    viewport: Viewport,
    url: &str,
    images: ImageRegistry,
) -> Result<(fortrust_renderer::RenderedPage, Option<String>), EngineError> {
    use fortrust_dom::DomArena;
    use fortrust_js::{EventLoop, JsRuntime, WebApiRegistry};

    let arena = Box::leak(Box::new(DomArena::new()));
    let document = fortrust_dom::parse_html(arena, html)?;
    let mut event_loop = EventLoop::new();
    let mut js = JsRuntime::new()
        .with_origin(url)
        .with_registry(WebApiRegistry::new());
    js.initialize(&mut event_loop)?;

    // Safety: the arena is intentionally leaked so it lives forever.
    let static_doc: &'static fortrust_dom::Document<'static> =
        unsafe { &*(&document as *const fortrust_dom::Document<'static>) };
    let _ = js.attach_document(static_doc);

    let scripts = extract_inline_scripts(html);
    for script in &scripts {
        if !script.trim().is_empty() {
            let _ = js.eval(script);
        }
    }

    // If scripts mutated the document title via our bindings, try to read it back.
    let js_title = match js.eval("document.getTitle()") {
        Ok(val) => val.to_string(js.context()).ok().map(|s| s.to_std_string_escaped()),
        Err(_) => None,
    };

    let renderer = fortrust_renderer::StaticRenderer::new();
    let rendered = renderer.render_document_with_images(&document, author_css, cosmetic_css, viewport, images)?;
    Ok((rendered, js_title))
}

#[cfg(not(feature = "javascript"))]
fn render_with_javascript(
    _html: &str,
    _author_css: &[&str],
    _cosmetic_css: &[&str],
    _viewport: Viewport,
    _url: &str,
    _images: ImageRegistry,
) -> Result<fortrust_renderer::RenderedPage, EngineError> {
    Err(EngineError::Render(
        fortrust_renderer::RenderError::EmptyDocument,
    ))
}

fn extract_inline_scripts(html: &str) -> Vec<String> {
    let mut scripts = Vec::new();
    let html_lower = html.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(start) = html_lower[pos..].find("<script") {
        let start_abs = pos + start;
        let tag_end = html_lower[start_abs..].find('>');
        let Some(tag_end) = tag_end else { break };
        let tag_end_abs = start_abs + tag_end + 1;

        let tag_content = &html[start_abs..tag_end_abs];
        if tag_content.to_ascii_lowercase().contains("src=\"")
            || tag_content.to_ascii_lowercase().contains("src='")
        {
            pos = tag_end_abs;
            continue;
        }

        let closing = "script>";
        let search_from = tag_end_abs;
        if let Some(end) = html_lower[search_from..].find(closing) {
            let end_abs = search_from + end;
            scripts.push(html[tag_end_abs..end_abs].to_owned());
            pos = end_abs + closing.len();
        } else {
            break;
        }
    }
    scripts
}

impl Default for TrustEngine {
    fn default() -> Self {
        Self::offline()
    }
}

fn title_from_html_or_url(html: &str, url: &str) -> String {
    let scan_end = html
        .char_indices()
        .take_while(|(index, _)| *index < 64 * 1024)
        .last()
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(html.len().min(64 * 1024));
    let head = &html[..scan_end];

    if let Some(start) = find_ascii_case_insensitive(head, "<title>")
        && let Some(end) = find_ascii_case_insensitive(&head[start + 7..], "</title>")
    {
        let title = head[start + 7..start + 7 + end].trim();
        if !title.is_empty() {
            return title.chars().take(80).collect();
        }
    }

    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(TRUST_ENGINE_NAME)
        .chars()
        .take(80)
        .collect()
}

async fn load_external_stylesheets(
    network: &mut NetworkClient,
    document_url: &str,
    html: &str,
) -> Result<(Vec<String>, usize), EngineError> {
    let hrefs = external_stylesheet_hrefs(document_url, html);
    if hrefs.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let mut loaded = Vec::new();
    let mut blocked_or_failed = 0usize;
    let mut remaining_budget = MAX_EXTERNAL_SUBRESOURCE_BYTES_PER_DOCUMENT;
    for href in hrefs {
        let response = network
            .fetch(RequestContext {
                url: href,
                top_level_url: Some(document_url.to_owned()),
                resource_type: ResourceType::Stylesheet,
                referrer_policy: None,
            })
            .await;

        match response {
            Ok(response) => {
                if !is_allowed_stylesheet_content_type(response.headers.get("content-type")) {
                    blocked_or_failed += 1;
                    continue;
                }

                match stylesheet_text_from_response(response.body).await {
                    Ok((css, css_bytes)) => {
                        if consume_document_budget(&mut remaining_budget, css_bytes).is_ok() {
                            loaded.push(css);
                        } else {
                            blocked_or_failed += 1;
                        }
                    }
                    Err(_) => blocked_or_failed += 1,
                }
            }
            Err(NetworkError::Blocked(_)) => blocked_or_failed += 1,
            Err(_) => blocked_or_failed += 1,
        }
    }

    Ok((loaded, blocked_or_failed))
}

async fn load_external_images(
    network: &NetworkClient,
    document_url: &str,
    html: &str,
) -> Result<(usize, usize, Vec<DecodedImage>), EngineError> {
    let hrefs = external_image_hrefs(document_url, html);
    if hrefs.is_empty() {
        return Ok((0, 0, Vec::new()));
    }

    let mut loaded = 0usize;
    let mut blocked_or_failed = 0usize;
    let mut decoded: Vec<DecodedImage> = Vec::new();
    let mut remaining_budget = MAX_EXTERNAL_SUBRESOURCE_BYTES_PER_DOCUMENT;
    for href in hrefs {
        let response = network
            .fetch_stream(RequestContext {
                url: href.clone(),
                top_level_url: Some(document_url.to_owned()),
                resource_type: ResourceType::Image,
                referrer_policy: None,
            })
            .await;

        match response {
            Ok(mut response) => {
                if !is_allowed_image_content_type(response.headers.get("content-type")) {
                    blocked_or_failed += 1;
                    continue;
                }

                let limit = remaining_budget.min(MAX_EXTERNAL_IMAGE_BYTES_PER_DOCUMENT);
                // Read up to the per-image limit so we can both count and decode.
                let mut buffer: Vec<u8> = Vec::new();
                let mut read_total: usize = 0;
                let mut truncated = false;
                while let Some(chunk) = response.body.next().await {
                    match chunk {
                        Ok(bytes) => {
                            if read_total + bytes.len() > limit {
                                let room = limit.saturating_sub(read_total);
                                buffer.extend_from_slice(&bytes[..room]);
                                read_total += room;
                                truncated = true;
                                break;
                            }
                            buffer.extend_from_slice(&bytes);
                            read_total += bytes.len();
                        }
                        Err(_) => {
                            blocked_or_failed += 1;
                            break;
                        }
                    }
                }
                if truncated || consume_document_budget(&mut remaining_budget, read_total).is_err() {
                    blocked_or_failed += 1;
                    continue;
                }

                // Attempt to decode. If decoding fails, count as a blocked/failed
                // image (we still try to limit unnecessary work).
                match image::load_from_memory(&buffer) {
                    Ok(dyn_img) => {
                        let rgba = dyn_img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        decoded.push(DecodedImage {
                            url: href,
                            width: w,
                            height: h,
                            rgba: rgba.into_raw(),
                        });
                        loaded += 1;
                    }
                    Err(e) => {
                        tracing::debug!(target: "fortrust.engine", "image decode failed for {href}: {e}");
                        blocked_or_failed += 1;
                    }
                }
            }
            Err(NetworkError::Blocked(_)) => blocked_or_failed += 1,
            Err(_) => blocked_or_failed += 1,
        }
    }

    Ok((loaded, blocked_or_failed, decoded))
}

fn external_stylesheet_hrefs(document_url: &str, html: &str) -> Vec<String> {
    let Ok(base_url) = Url::parse(document_url) else {
        return Vec::new();
    };

    let arena = DomArena::new();
    let Ok(document) = parse_html(&arena, html) else {
        return Vec::new();
    };

    document
        .descendants()
        .into_iter()
        .filter_map(|node| stylesheet_href(node, &base_url))
        .take(MAX_EXTERNAL_STYLESHEETS_PER_DOCUMENT)
        .collect()
}

fn external_image_hrefs(document_url: &str, html: &str) -> Vec<String> {
    let Ok(base_url) = Url::parse(document_url) else {
        return Vec::new();
    };

    let arena = DomArena::new();
    let Ok(document) = parse_html(&arena, html) else {
        return Vec::new();
    };

    document
        .descendants()
        .into_iter()
        .filter_map(|node| image_href(node, &base_url))
        .take(MAX_EXTERNAL_IMAGES_PER_DOCUMENT)
        .collect()
}

fn stylesheet_href(node: NodeRef<'_>, base_url: &Url) -> Option<String> {
    let element = node.as_element()?;
    if !element.local_name().eq_ignore_ascii_case("link") {
        return None;
    }

    let rel = element.attr("rel")?;
    if !rel
        .split_ascii_whitespace()
        .any(|part| part.eq_ignore_ascii_case("stylesheet"))
    {
        return None;
    }

    let href = element.attr("href")?;
    let url = base_url.join(href.trim()).ok()?;
    if matches!(url.scheme(), "http" | "https") {
        Some(url.to_string())
    } else {
        None
    }
}

fn image_href(node: NodeRef<'_>, base_url: &Url) -> Option<String> {
    let element = node.as_element()?;
    if !element.local_name().eq_ignore_ascii_case("img") {
        return None;
    }

    let src = element.attr("src")?;
    let url = base_url.join(src.trim()).ok()?;
    if matches!(url.scheme(), "http" | "https") {
        Some(url.to_string())
    } else {
        None
    }
}

async fn stylesheet_text_from_response(
    body: bytes::Bytes,
) -> Result<(String, usize), NetworkError> {
    if body.len() > MAX_EXTERNAL_STYLESHEET_BYTES_PER_DOCUMENT {
        return Err(NetworkError::BodyTooLarge {
            limit_bytes: MAX_EXTERNAL_STYLESHEET_BYTES_PER_DOCUMENT,
        });
    }

    std::str::from_utf8(&body)
        .map(|text| (text.to_owned(), body.len()))
        .map_err(|_| NetworkError::Transport("stylesheet body was not valid UTF-8".to_owned()))
}

fn consume_document_budget(
    remaining_budget: &mut usize,
    consumed_bytes: usize,
) -> Result<(), NetworkError> {
    if consumed_bytes > *remaining_budget {
        return Err(NetworkError::BodyTooLarge {
            limit_bytes: *remaining_budget,
        });
    }

    *remaining_budget -= consumed_bytes;
    Ok(())
}

fn is_allowed_stylesheet_content_type(value: Option<&http::HeaderValue>) -> bool {
    let Some(value) = value.and_then(|header: &http::HeaderValue| header.to_str().ok()) else {
        return false;
    };

    matches!(
        normalized_content_type(value).as_str(),
        "text/css" | "application/css"
    )
}

fn is_allowed_image_content_type(value: Option<&http::HeaderValue>) -> bool {
    let Some(value) = value.and_then(|header: &http::HeaderValue| header.to_str().ok()) else {
        return false;
    };

    matches!(
        normalized_content_type(value).as_str(),
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
    )
}

fn normalized_content_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| {
            window
                .iter()
                .zip(needle.as_bytes())
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        })
}

fn internal_html(url: &str) -> String {
    format!(
        r#"<!doctype html>
<html>
<head>
  <title>{TRUST_ENGINE_NAME}</title>
  <style>
    body {{ background-color: #ffffff; color: #111111; margin: 16px; }}
    main {{ background-color: #eef7f4; padding: 16px; }}
    h1 {{ color: #0f766e; font-size: 32px; margin: 8px; }}
    p {{ margin: 8px; }}
    strong {{ color: #0f766e; }}
  </style>
</head>
<body>
  <main>
    <h1>{TRUST_ENGINE_NAME}</h1>
    <p>Secure static rendering is online for <strong>{}</strong>.</p>
    <p>JavaScript and external subresources remain disabled until isolated renderer and privacy-gated resource loading are complete.</p>
  </main>
</body>
</html>"#,
        escape_text(url)
    )
}

fn escape_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_has_public_name() {
        assert_eq!(TRUST_ENGINE_NAME, "Trust Engine");
    }

    #[test]
    fn renders_html_with_embedded_css() {
        let page = TrustEngine::offline()
            .render_html(
                "trust://test",
                "<title>Trust Test</title><style>p { color: blue; }</style><body><p>Hello</p></body>",
                &[],
                Viewport { width: 320.0, height: 200.0 },
            )
            .unwrap();

        assert_eq!(page.title, "Trust Test");
        assert!(page
            .rendered
            .display_list
            .commands()
            .iter()
            .any(|command| matches!(
                command,
                DisplayCommand::DrawText {
                    text,
                    color: Color { r: 0, g: 0, b: 255, a: 255 },
                    ..
                } if text == "Hello"
            )));

        assert!(page.security.javascript_enabled);
        assert!(page.security.sandboxed_static_render);
        assert!(page.security.body_bytes > 0);
        assert!(page.security.display_commands > 0);
    }

    #[test]
    fn renders_borders_through_full_pipeline() {
        let page = TrustEngine::offline()
            .render_html(
                "trust://test",
                r#"<div style="border: 3px solid #00ff00; width: 100px; height: 60px;">Green border</div>"#,
                &[],
                Viewport { width: 320.0, height: 200.0 },
            )
            .unwrap();

        let has_border = page.rendered.display_list.commands().iter().any(|command| {
            matches!(
                command,
                DisplayCommand::DrawBorder {
                    top_width,
                    top_color: Color { r: 0, g: 255, b: 0, a: 255 },
                    ..
                } if *top_width > 0.0
            )
        });
        assert!(has_border, "Full pipeline: CSS border should produce DrawBorder command");
    }

    #[test]
    fn offline_engine_refuses_network_loads() {
        let mut engine = TrustEngine::offline();
        let error = futures_executor::block_on(engine.load_url(
            "https://example.com",
            Viewport {
                width: 320.0,
                height: 200.0,
            },
        ))
        .unwrap_err();

        assert_eq!(error, EngineError::NetworkUnavailable);
    }

    #[test]
    fn discovers_external_stylesheets_with_url_resolution() {
        let hrefs = external_stylesheet_hrefs(
            "https://example.com/app/index.html",
            r#"
            <link rel="preload" href="/ignored.css">
            <link rel="stylesheet" href="/site.css">
            <link rel="alternate stylesheet" href="theme/dark.css">
            <link rel="stylesheet" href="data:text/css,body{}">
            "#,
        );

        assert_eq!(
            hrefs,
            vec![
                "https://example.com/site.css".to_owned(),
                "https://example.com/app/theme/dark.css".to_owned(),
            ]
        );
    }

    #[test]
    fn stylesheet_discovery_is_bounded() {
        let mut html = String::new();
        for index in 0..MAX_EXTERNAL_STYLESHEETS_PER_DOCUMENT + 4 {
            html.push_str(&format!(
                r#"<link rel="stylesheet" href="/style-{index}.css">"#
            ));
        }

        let hrefs = external_stylesheet_hrefs("https://example.com/", &html);
        assert_eq!(hrefs.len(), MAX_EXTERNAL_STYLESHEETS_PER_DOCUMENT);
    }

    #[test]
    fn discovers_external_images_with_url_resolution() {
        let hrefs = external_image_hrefs(
            "https://example.com/app/index.html",
            r#"
            <img src="/logo.png" alt="Logo">
            <img src="images/photo.jpg">
            <img src="data:image/png;base64,AAAA">
            <img src="javascript:alert(1)">
            "#,
        );

        assert_eq!(
            hrefs,
            vec![
                "https://example.com/logo.png".to_owned(),
                "https://example.com/app/images/photo.jpg".to_owned(),
            ]
        );
    }

    #[test]
    fn image_discovery_is_bounded() {
        let mut html = String::new();
        for index in 0..MAX_EXTERNAL_IMAGES_PER_DOCUMENT + 4 {
            html.push_str(&format!(r#"<img src="/image-{index}.png">"#));
        }

        let hrefs = external_image_hrefs("https://example.com/", &html);
        assert_eq!(hrefs.len(), MAX_EXTERNAL_IMAGES_PER_DOCUMENT);
    }

    #[test]
    fn stylesheet_content_type_validation_is_fail_closed() {
        use http::HeaderValue;

        assert!(is_allowed_stylesheet_content_type(Some(
            &HeaderValue::from_static("text/css; charset=utf-8")
        )));
        assert!(!is_allowed_stylesheet_content_type(Some(
            &HeaderValue::from_static("text/html")
        )));
        assert!(!is_allowed_stylesheet_content_type(None));
    }

    #[test]
    fn stylesheet_bytes_are_bounded() {
        use bytes::Bytes;

        let body = Bytes::from(vec![b'a'; MAX_EXTERNAL_STYLESHEET_BYTES_PER_DOCUMENT + 1]);
        let error = futures_executor::block_on(stylesheet_text_from_response(body)).unwrap_err();
        assert_eq!(
            error,
            NetworkError::BodyTooLarge {
                limit_bytes: MAX_EXTERNAL_STYLESHEET_BYTES_PER_DOCUMENT,
            }
        );
    }

    #[test]
    fn document_budget_is_consumed_across_subresources() {
        let mut remaining_budget = 8usize;

        assert!(consume_document_budget(&mut remaining_budget, 5).is_ok());
        assert_eq!(remaining_budget, 3);

        let error = consume_document_budget(&mut remaining_budget, 4).unwrap_err();
        assert_eq!(error, NetworkError::BodyTooLarge { limit_bytes: 3 });
    }

    #[test]
    fn image_content_type_validation_is_fail_closed() {
        use http::HeaderValue;

        assert!(is_allowed_image_content_type(Some(
            &HeaderValue::from_static("image/png")
        )));
        assert!(is_allowed_image_content_type(Some(
            &HeaderValue::from_static("image/jpeg; charset=binary")
        )));
        assert!(!is_allowed_image_content_type(Some(
            &HeaderValue::from_static("application/octet-stream")
        )));
        assert!(!is_allowed_image_content_type(None));
    }
}
