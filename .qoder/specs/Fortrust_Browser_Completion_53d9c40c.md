# Fortrust Browser — Full Completion Plan

## Context

Fortrust is a Rust-based privacy browser at ~40-50% completion (19,600 LOC, 14 crates, 131 passing tests). The static rendering pipeline works end-to-end (DOM→Style→Layout→Paint), privacy filtering is functional, storage/history/bookmarks persist, and the egui browser shell is interactive with tabs, omnibox, sidebar, and shield. 

**Critical gaps preventing production use:** text rendering shows colored rectangles instead of glyphs, DOM mutation is read-only (no JS content updates), fetch() is stubbed, multi-process isolation isn't active, and many Web APIs are missing. The goal is to complete ALL of these while adding container identity isolation and zero-knowledge sync as unique differentiators, with a visually stunning UI.

**Architecture:** Multi-process (browser chrome + renderer + network service), hybrid JS engine (V8 for performance tabs, Boa for sandboxed/privacy contexts), egui-based chrome UI with wgpu acceleration.

---

## Phase 1: Visual Foundation — Text & Font Rendering (Week 1-2)

**Goal:** Make pages readable with real text rendering.

### Task 1.1: Wire cosmic-text into the Paint Pipeline
- Connect `cosmic-text 0.12` + `swash 0.1` (already in Cargo.toml) to `fortrust-paint`
- Replace all `PaintCmd::Text` placeholder rectangles with actual glyph rasterization
- Implement font fallback chain (system fonts → bundled fallback)
- **Files:** `crates/fortrust-paint/src/lib.rs`, `bins/renderer/src/main.rs`

### Task 1.2: @font-face Support
- Parse `@font-face` rules in `fortrust-style` CSS cascade
- Download font files via `fortrust-net` during subresource discovery
- Register downloaded fonts with cosmic-text FontSystem
- **Files:** `crates/fortrust-style/src/lib.rs`, `crates/fortrust-renderer/src/lib.rs`

### Task 1.3: Text Layout Integration
- Wire text metrics (line height, baseline, advance widths) into `fortrust-layout`
- Support text wrapping, word-break, overflow-wrap
- Handle inline text with mixed styles (bold, italic, font-size changes)
- **Files:** `crates/fortrust-layout/src/lib.rs`

---

## Phase 2: Dynamic Content — DOM Mutation & Events (Week 3-6)

**Goal:** Make JavaScript-driven pages functional.

### Task 2.1: DOM Mutation API
- Implement `appendChild`, `removeChild`, `insertBefore`, `replaceChild` on arena DOM
- Implement `textContent` setter, `innerHTML` setter (with html5ever re-parse)
- Implement `createElement`, `createTextNode`, `setAttribute`
- Add MutationObserver notification hooks
- **Files:** `crates/fortrust-dom/src/lib.rs`, `crates/fortrust-dom/tests/mutation.rs`

### Task 2.2: Event System
- Implement EventTarget (addEventListener, removeEventListener, dispatchEvent)
- Event propagation: capture → target → bubble phases
- Standard events: click, input, change, submit, keydown, keyup, focus, blur, scroll
- Event objects with target, currentTarget, preventDefault, stopPropagation
- **Files:** `crates/fortrust-dom/src/lib.rs` (new event module), `crates/fortrust-js/src/bindings/dom_api.rs`

### Task 2.3: DOM-JS Bidirectional Bridge
- Expose mutation APIs to Boa runtime via JS bindings
- Implement live NodeList/HTMLCollection updates on mutation
- Trigger re-style → re-layout → re-paint on DOM changes (incremental)
- **Files:** `crates/fortrust-js/src/bindings/dom_api.rs`, `crates/fortrust-js/src/runtime.rs`

### Task 2.4: Incremental Re-render Pipeline
- Dirty-flag system: mark affected subtrees on mutation
- Incremental style recalculation (only dirty nodes)
- Incremental layout (only affected boxes)
- Partial repaint (damage rectangles)
- **Files:** `crates/fortrust-renderer/src/lib.rs`, `crates/trust-engine/src/lib.rs`

---

## Phase 3: Core Web APIs (Week 7-10)

**Goal:** Support AJAX-driven modern websites.

### Task 3.1: fetch() API — Full Implementation
- Connect JS `fetch()` binding to `fortrust-net` client
- Implement Request/Response objects with headers, body, status
- Handle CORS (preflight OPTIONS, allowed headers/origins)
- Support streaming responses (ReadableStream basics)
- AbortController/AbortSignal integration
- **Files:** `crates/fortrust-js/src/bindings/fetch.rs`, `crates/fortrust-net/src/client.rs`

### Task 3.2: XMLHttpRequest (Legacy Compat)
- Implement XMLHttpRequest for legacy sites
- States: UNSENT → OPENED → HEADERS_RECEIVED → LOADING → DONE
- Event callbacks: onreadystatechange, onload, onerror
- **Files:** `crates/fortrust-js/src/bindings/` (new `xhr.rs`)

### Task 3.3: WebSocket API
- Wire `tokio-tungstenite` into `fortrust-net/src/websocket.rs`
- Expose WebSocket constructor to JS (new WebSocket(url))
- Events: onopen, onmessage, onclose, onerror
- Binary/text frame support
- **Files:** `crates/fortrust-net/src/websocket.rs`, `crates/fortrust-js/src/bindings/websocket.rs`

### Task 3.4: Web Storage & IndexedDB
- localStorage/sessionStorage already working — verify completeness
- Implement IndexedDB (object stores, cursors, transactions) using redb backend
- **Files:** `crates/fortrust-js/src/bindings/storage.rs`, `crates/fortrust-storage/src/lib.rs`

### Task 3.5: Form Handling
- Form submission (GET/POST with urlencoded/multipart)
- Input validation (required, pattern, type constraints)
- File input handling
- **Files:** `crates/fortrust-dom/src/lib.rs`, `crates/fortrust-net/src/client.rs`

---

## Phase 4: Hybrid JavaScript Engine (Week 11-13)

**Goal:** V8 for performance, Boa for sandboxed privacy contexts.

### Task 4.1: V8 Runtime Integration
- Initialize `rusty_v8` isolate with proper heap configuration
- Create V8 execution context with global object bindings
- Wire all existing Web API bindings (DOM, fetch, timers, storage) to V8 context
- **Files:** `crates/fortrust-js/src/runtime.rs` (new v8_runtime module)

### Task 4.2: Engine Selection Logic
- Per-tab engine selection: V8 (default) vs Boa (privacy/sandbox mode)
- Container tabs use Boa by default for isolation
- Regular tabs use V8 for performance
- Seamless API surface — same bindings regardless of engine
- **Files:** `crates/fortrust-js/src/lib.rs`, `crates/fortrust-core/src/tabs.rs`

### Task 4.3: Boa Sandbox Hardening
- Restrict Boa contexts: no access to system APIs
- Memory limits per Boa instance
- Execution time limits (kill after timeout)
- Use for extension sandboxing and privacy containers
- **Files:** `crates/fortrust-js/src/runtime.rs`

---

## Phase 5: Multi-Process Architecture (Week 14-17)

**Goal:** Crash isolation, security sandboxing, true process separation.

### Task 5.1: Process Spawning & Lifecycle
- Browser process spawns renderer and netproc on startup
- Per-tab renderer process (or process-per-site-instance)
- Process crash detection and automatic restart
- Graceful shutdown coordination
- **Files:** `bins/fortrust/src/main.rs`, `crates/fortrust-ipc/src/lib.rs`

### Task 5.2: IPC Protocol Completion
- Complete the bincode-over-TCP IPC protocol
- Message types: NavigateTo, DomReady, PaintBuffer, FetchRequest, FetchResponse, JsEval, InputEvent
- Shared memory for paint buffers (avoid copying pixel data)
- **Files:** `crates/fortrust-ipc/src/messages.rs`, `crates/fortrust-ipc/src/channel.rs`

### Task 5.3: Renderer Process Isolation
- Renderer runs in restricted sandbox (no filesystem, limited network)
- All network requests routed through browser process → netproc
- Paint buffers transmitted via shared memory to browser process
- **Files:** `bins/renderer/src/main.rs`, `crates/fortrust-renderer/src/lib.rs`

### Task 5.4: GPU Process (wgpu)
- Dedicated GPU process for wgpu-accelerated compositing
- Receive paint buffers from renderer processes
- Hardware-accelerated layer compositing
- WebGL/WebGPU forwarding
- **Files:** new `bins/gpuproc/` binary, `crates/fortrust-paint/src/lib.rs`

---

## Phase 6: CSS Completeness & Animations (Week 18-20)

**Goal:** Modern CSS rendering fidelity.

### Task 6.1: CSS Animations & Transitions
- Wire @keyframes to render loop frame ticks
- Property interpolation (color, transform, opacity, dimensions)
- CSS transitions (transition-property, duration, timing-function, delay)
- requestAnimationFrame support
- **Files:** `crates/fortrust-chrome/src/animation.rs`, `crates/fortrust-style/src/lib.rs`

### Task 6.2: CSS Grid Layout
- Implement CSS Grid (grid-template-columns/rows, grid-area, gap)
- Auto-placement algorithm
- Named grid lines and areas
- **Files:** `crates/fortrust-layout/src/lib.rs`

### Task 6.3: CSS Advanced Features
- CSS variables (custom properties with var())
- CSS transforms (translate, rotate, scale, matrix)
- CSS filters (blur, brightness, contrast, grayscale)
- position: sticky
- overflow: scroll with scrollbar rendering
- **Files:** `crates/fortrust-style/src/lib.rs`, `crates/fortrust-layout/src/lib.rs`, `crates/fortrust-paint/src/lib.rs`

### Task 6.4: Media Queries & Responsive Design
- Viewport-based media queries
- prefers-color-scheme, prefers-reduced-motion
- Container queries (basic support)
- **Files:** `crates/fortrust-style/src/lib.rs`

---

## Phase 7: Container Identity Isolation — Key Differentiator (Week 21-23)

**Goal:** Per-container isolated browsing identities (superior to Firefox containers and Brave).

### Task 7.1: Container Architecture
- Container data model: ID, name, color, icon, fingerprint profile, network config
- Storage partitioning by container_id + origin (cookies, localStorage, IndexedDB, cache)
- Separate service worker registrations per container
- **Files:** `crates/fortrust-core/src/workspaces.rs` (extend), `crates/fortrust-storage/src/lib.rs`

### Task 7.2: Per-Container Fingerprint Spoofing
- Unique user-agent per container
- Canvas noise injection (per-container seed)
- WebGL renderer/vendor spoofing
- Screen resolution spoofing
- Timezone/locale randomization per container
- Font list spoofing
- **Files:** `crates/fortrust-privacy/src/fingerprint.rs`, `crates/fortrust-js/src/bindings/navigator.rs`, `crates/fortrust-js/src/bindings/screen.rs`

### Task 7.3: Container Network Isolation
- Per-container DNS resolver (different DoH providers)
- Per-container proxy configuration
- Per-container cookie jar (fully isolated)
- No cross-container cookie/storage leaks
- **Files:** `crates/fortrust-net/src/dns.rs`, `crates/fortrust-net/src/client.rs`, `crates/fortrust-privacy/src/cookie_policy.rs`

### Task 7.4: Container UI
- Color-coded tab indicators per container
- Container picker in new-tab menu
- Container management panel in settings
- Visual separation between container contexts
- **Files:** `crates/fortrust-chrome/src/sidebar.rs`, `crates/fortrust-chrome/src/app.rs`

---

## Phase 8: Zero-Knowledge Sync — Key Differentiator (Week 24-26)

**Goal:** Encrypted cross-device sync where the server cannot read user data.

### Task 8.1: Encryption Layer
- Key derivation from user password via Argon2 (already in deps)
- AES-256-GCM encryption for all sync payloads
- Per-device key rotation support
- Recovery key generation (offline backup)
- **Files:** new `crates/fortrust-sync/` crate

### Task 8.2: Sync Protocol
- Define sync protocol (CRDT-based conflict resolution)
- Sync entities: bookmarks, history, settings, passwords, containers, open tabs
- Incremental sync (only changes since last sync)
- Encrypted metadata (server sees encrypted blobs + timestamps only)
- **Files:** `crates/fortrust-sync/src/protocol.rs`, `crates/fortrust-sync/src/crdt.rs`

### Task 8.3: Sync Server (Minimal)
- Simple Rust HTTP server (axum) storing encrypted blobs
- Authentication via proof-of-knowledge (no plaintext passwords)
- Rate limiting and storage quotas
- Self-hostable (Docker image)
- **Files:** new `bins/sync-server/` binary

### Task 8.4: Sync UI
- Account creation/login in settings
- Sync status indicator in chrome
- Conflict resolution UI (if needed)
- Device management panel
- **Files:** `crates/fortrust-chrome/src/app.rs`, `crates/fortrust-chrome/src/sidebar.rs`

---

## Phase 9: Visually Stunning UI/UX (Week 27-30)

**Goal:** Make Fortrust look and feel premium — better than Brave's chrome.

### Task 9.1: Custom Theme Engine
- Dynamic theming system with smooth color transitions
- Glassmorphism/frosted glass effects on panels
- Accent color extraction from active page
- Dark/light/auto mode with smooth transitions
- Custom font (Inter/JetBrains Mono) for browser chrome
- **Files:** `crates/fortrust-chrome/src/theme.rs`, `crates/fortrust-chrome/src/app.rs`

### Task 9.2: Animated Transitions & Micro-interactions
- Tab open/close animations (smooth slide/fade)
- Sidebar expand/collapse with spring animation
- Page loading animation (custom progress indicator, not boring bar)
- Shield blocking animation (particles/ripple on blocked request)
- Smooth scrolling with momentum
- **Files:** `crates/fortrust-chrome/src/animation.rs`, `crates/fortrust-chrome/src/sidebar.rs`

### Task 9.3: Advanced Tab UI
- Vertical tabs with thumbnails (live preview)
- Tab groups with color-coded headers and collapse
- Tab search/filter overlay
- Tree-style tab hierarchy (parent-child)
- Split-view (two pages side-by-side)
- Picture-in-picture tab detach
- **Files:** `crates/fortrust-chrome/src/app.rs`, `crates/fortrust-chrome/src/sidebar.rs`

### Task 9.4: Speed Dial & New Tab Page
- Customizable speed dial grid with site favicons
- Background images (daily rotation or user-set)
- Quick-access widgets (weather, bookmarks, recently closed)
- Privacy stats dashboard (trackers blocked today, cookies prevented)
- Motivational privacy tips
- **Files:** `crates/fortrust-chrome/src/speed_dial.rs`, `crates/fortrust-chrome/src/backgrounds.rs`

### Task 9.5: Omnibox & Command Palette
- Rich autocomplete with favicon + title + URL
- Inline search suggestions (from privacy search engine)
- Command palette (Ctrl+K) for browser actions
- Quick switch between tabs/bookmarks/history
- **Files:** `crates/fortrust-chrome/src/omnibox.rs`

---

## Phase 10: Search Engine Integration (Week 31-33)

**Goal:** Built-in privacy-preserving metasearch.

### Task 10.1: Metasearch Aggregation Engine
- Parallel query to multiple backends (DuckDuckGo, Brave, Mojeek, SearXNG)
- Result deduplication and merging
- Local relevance ranking (no server-side personalization)
- Query anonymization (strip identifiers, rotate source IPs via proxy)
- **Files:** `crates/fortrust-search/src/lib.rs`

### Task 10.2: Search Results Page
- Custom SERP rendered natively (not HTML page)
- Rich snippets, image results, knowledge panels
- Instant answers (calculator, conversions, definitions)
- Source attribution with privacy indicators
- **Files:** `crates/fortrust-chrome/src/app.rs` (search results view)

### Task 10.3: Search Suggestions & Autocomplete
- Local history-based suggestions (no server queries for suggestions)
- Trending topics (fetched anonymously, cached locally)
- Bookmark/tab search integration in omnibox
- **Files:** `crates/fortrust-search/src/lib.rs`, `crates/fortrust-chrome/src/omnibox.rs`

---

## Phase 11: Media, Canvas & Advanced APIs (Week 34-37)

**Goal:** Support multimedia-rich websites.

### Task 11.1: Canvas 2D API
- Implement CanvasRenderingContext2D (paths, fills, strokes, text, images)
- Software rasterization backend (with wgpu acceleration option)
- toDataURL() and toBlob() export
- **Files:** `crates/fortrust-js/src/bindings/` (new `canvas.rs`), `crates/fortrust-paint/src/lib.rs`

### Task 11.2: HTML5 Video/Audio
- `<video>` and `<audio>` element support
- System codec integration (platform media frameworks)
- HTMLMediaElement API (play, pause, seek, volume, events)
- Fullscreen video support
- **Files:** `crates/fortrust-dom/src/lib.rs`, new media module

### Task 11.3: Web Workers
- Dedicated Worker (spawn background JS thread)
- Shared Worker (cross-tab communication)
- Message passing via postMessage/onmessage
- **Files:** `crates/fortrust-js/src/` (new `workers.rs`)

### Task 11.4: Service Workers & Offline Support
- Service Worker registration and lifecycle
- Cache API for offline content
- Intercept fetch events
- Push notifications (basic)
- **Files:** `crates/fortrust-js/src/` (new `service_worker.rs`), `crates/fortrust-net/src/cache.rs`

---

## Phase 12: Extensions & DevTools (Week 38-42)

**Goal:** WebExtensions compatibility and developer tools.

### Task 12.1: WebExtensions API (Core Subset)
- manifest.json parsing (Manifest V3)
- Content scripts (inject JS/CSS into pages)
- Background service workers
- Browser action (popup UI)
- APIs: tabs, storage, cookies, webRequest/declarativeNetRequest, bookmarks, history
- Extension isolation (separate V8 context per extension)
- **Files:** new `crates/fortrust-extensions/` crate

### Task 12.2: Extension Marketplace UI
- Extension management page (installed, enable/disable, remove)
- Sideload extensions from .zip/.crx
- Permission review before install
- **Files:** `crates/fortrust-chrome/src/app.rs`

### Task 12.3: DevTools — Inspector
- DOM tree inspector (element selection, style display)
- Computed styles panel
- Box model visualization
- **Files:** new `crates/fortrust-devtools/` crate

### Task 12.4: DevTools — Console & Network
- JavaScript console (eval, log display, error stack traces)
- Network panel (request list, headers, timing, payload)
- Performance timeline (basic)
- **Files:** `crates/fortrust-devtools/src/`

---

## Phase 13: Security Hardening & Advanced Privacy (Week 43-46)

**Goal:** Production-grade security posture.

### Task 13.1: Renderer Sandbox
- OS-level sandboxing (Windows: restricted tokens, job objects)
- Syscall filtering
- No direct file/network access from renderer
- **Files:** `bins/renderer/src/main.rs`, new sandbox module

### Task 13.2: Encrypted Client Hello (ECH)
- Implement ECH in rustls TLS handshake
- Encrypt SNI field to prevent network observers from seeing destinations
- Automatic fallback when ECH not supported
- **Files:** `crates/fortrust-net/src/tls.rs`

### Task 13.3: Advanced Fingerprint Resistance
- Audio context fingerprint protection
- Battery API spoofing
- Hardware concurrency spoofing
- WebRTC IP leak prevention
- Keyboard/typing pattern noise
- **Files:** `crates/fortrust-privacy/src/fingerprint.rs`

### Task 13.4: Safe Browsing Integration
- Phishing URL detection (local bloom filter from Safe Browsing lists)
- Malware download blocking
- Warning interstitial pages
- **Files:** `crates/fortrust-privacy/src/` (new `safe_browsing.rs`)

---

## Phase 14: Password Manager & Autofill (Week 47-48)

**Goal:** Secure credential management.

### Task 14.1: Password Vault
- Encrypted credential storage (AES-256-GCM, master password derived key)
- Form field detection (username/password heuristics)
- Save prompt on form submission
- Autofill on recognized login pages
- **Files:** new `crates/fortrust-credentials/` crate

### Task 14.2: Password Generator
- Configurable random password generation
- Strength indicator
- Integration with save prompt
- **Files:** `crates/fortrust-credentials/src/`

---

## Phase 15: HTTP/3, WebRTC & Final APIs (Week 49-52)

**Goal:** Complete protocol support and remaining APIs.

### Task 15.1: HTTP/3 (QUIC)
- Wire `quinn` + `h3` into transport layer
- ALPN negotiation (h3 → h2 → h1.1 fallback)
- 0-RTT resumption
- **Files:** `crates/fortrust-net/src/transport.rs`

### Task 15.2: WebRTC (Basic)
- RTCPeerConnection with ICE candidate gathering
- STUN/TURN client
- Basic audio/video stream handling
- Privacy: IP leak prevention via relay-only mode
- **Files:** new module in `crates/fortrust-net/`

### Task 15.3: Headless Mode & Automation
- `--headless` offscreen rendering with PNG export
- Automation protocol (CDP-compatible subset)
- CI screenshot testing
- **Files:** `bins/fortrust/src/main.rs`

### Task 15.4: PDF Viewer
- Integrate PDF rendering (pdf-rs or similar Rust crate)
- In-tab PDF display with zoom, search, page navigation
- **Files:** new module or integration in renderer

---

## Key Files Reference

| Module | Primary Files |
|--------|--------------|
| Browser Shell | `crates/fortrust-chrome/src/app.rs`, `theme.rs`, `sidebar.rs`, `omnibox.rs` |
| DOM Engine | `crates/fortrust-dom/src/lib.rs` |
| JS Engine | `crates/fortrust-js/src/runtime.rs`, `bindings/*` |
| Layout | `crates/fortrust-layout/src/lib.rs` |
| Paint | `crates/fortrust-paint/src/lib.rs` |
| Style | `crates/fortrust-style/src/lib.rs` |
| Network | `crates/fortrust-net/src/client.rs`, `dns.rs`, `tls.rs`, `websocket.rs` |
| Privacy | `crates/fortrust-privacy/src/fingerprint.rs`, `blocker.rs`, `cookie_policy.rs` |
| Storage | `crates/fortrust-storage/src/lib.rs` |
| IPC | `crates/fortrust-ipc/src/messages.rs`, `channel.rs` |
| Engine | `crates/trust-engine/src/lib.rs` |
| Main Binary | `bins/fortrust/src/main.rs` |
| Renderer | `bins/renderer/src/main.rs` |
| Net Process | `bins/netproc/src/main.rs` |

---

## Verification Strategy

### Per-Phase Testing
- **Unit tests**: Each new API gets unit tests (target: 500+ tests total)
- **Integration tests**: `trust-engine` e2e tests loading real HTML fixtures
- **Visual regression**: Headless screenshots compared against reference images
- **JS compliance**: Run test262 subset for implemented APIs

### End-to-End Milestones
- **Phase 2 complete**: Load and interact with a simple todo-app (DOM mutation + events)
- **Phase 3 complete**: Load Twitter/GitHub login pages (fetch + forms)
- **Phase 5 complete**: Crash one tab without losing others
- **Phase 7 complete**: Browse in two containers with verified isolation (different fingerprints)
- **Phase 9 complete**: Screenshot comparison with Brave showing competitive UI quality
- **Phase 12 complete**: Install and run uBlock Origin extension

### Performance Targets
- Cold start: < 800ms to interactive
- Memory baseline: < 150MB (no tabs)
- Per-tab overhead: < 30MB average
- Page load (static): < 500ms for simple pages
- Privacy filtering: < 5ms overhead per request

---

## Implementation Notes for Solo Developer

- **Work in phases sequentially** — each phase builds on previous
- **Ship internal milestones** — Phase 2 completion = first "real" browser moment
- **Test continuously** — `cargo test --workspace` after every major change
- **Commit frequently** — one logical commit per task
- **Priority if stuck**: Skip to next task in phase, return later
- **Estimated total timeline**: 52 weeks (1 year) at full-time pace, ~18-24 months at part-time
