# Fortrust

<p align="center">
	<img src="assets/Fortrust-Banner.png" alt="Fortrust banner" width="920" />
</p>

<p align="center">
	<img src="assets/Fortrust-Logo.png" alt="Fortrust logo" width="180" />
</p>

<p align="center">
	<b>Fortrust</b> is a Rust privacy browser foundation built for strict defaults, calm chrome, and low-memory tab behavior.
</p>

## What it is

Fortrust is a compact Rust workspace for a privacy-first browser stack. It already includes request filtering, secure networking, DOM parsing, CSS, layout, painting, static rendering, a Trust Engine facade, and a native egui browser shell.

## What stands out

| Area | Summary |
|---|---|
| Privacy | Upgrades HTTP, blocks known tracker/ad hosts, strips tracking parameters, and tags third-party requests. |
| Network | Uses rustls/WebPKI TLS, DoH selection, HTTP cache validation, and buffered or streaming fetch paths. |
| Rendering | Parses HTML into an arena DOM, applies CSS, builds layout boxes, and emits a display list. |
| Browser shell | Native egui chrome with tab state, history, navigation, memory budgeting, and a background engine worker. |

## Architecture at a glance

```text
					+----------------------+
					|   fortrust-chrome    |
					|  native egui shell   |
					+----------+-----------+
										 |
										 v
					+----------------------+
					|     trust-engine     |
					| secure render facade |
					+--+----+----+----+----+
						 |    |    |    |    |
						 |    |    |    |    +----> fortrust-core
						 |    |    |    +---------> fortrust-renderer
						 |    |    +--------------> fortrust-paint
						 |    +-------------------> fortrust-layout
						 +------------------------> fortrust-net

fortrust-renderer -> fortrust-dom -> fortrust-style -> fortrust-layout -> fortrust-paint
```

## Module Map

```text
fortrust-core     privacy rules, tab state, config
fortrust-net      TLS, DoH, cache, request transport
fortrust-dom      arena-backed HTML DOM
fortrust-style    CSS cascade and computed values
fortrust-layout   block, inline, text, and flex-aware boxes
fortrust-paint    viewport-clipped display list
fortrust-renderer static HTML to paint pipeline
trust-engine      secure page loading facade
fortrust-chrome   native browser UI and tab worker
```

## Run

```powershell
cargo run -p fortrust
```

## Test

```powershell
cargo test
```

## Design Notes

- Privacy comes first: every navigation is inspected before it leaves the browser shell.
- Tabs stay lightweight: inactive tabs are kept cheap instead of keeping the whole world awake.
- The UI stays minimal: dark chrome, clear state, and no extra fluff.
- Trust Engine stays separate so the secure load pipeline can be reused later.
