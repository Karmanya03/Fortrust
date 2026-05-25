# Fortrust Security Model

Fortrust starts from strict defaults. The current implementation is a policy foundation, not a completed browser sandbox.

## Current Protections

- HTTP navigations are upgraded to HTTPS when possible.
- Mixed active content is blocked when loaded below an HTTPS top-level page.
- Known tracker and ad host patterns are blocked before navigation.
- Tracking query parameters such as `utm_*`, `fbclid`, and `gclid` are stripped.
- Third-party cookie blocking is modeled and surfaced in the shield panel.
- Global Privacy Control and Do Not Track notes are applied to allowed decisions.
- Fingerprint noise uses a per-profile salt and origin/day scoped seed.

## Required Before Real-World Browsing

- Use a dedicated network process with rustls certificate verification.
- Use platform sandboxing or job objects for renderer isolation.
- Enforce content security policy and permission prompts in the renderer.
- Replace built-in sample filters with continuously updated EasyList/EasyPrivacy style lists.
- Add fuzzing for URL parsing, request policy, DOM parsing, CSS parsing, IPC, and storage codecs.
- Treat every parser and renderer boundary as hostile input.

## Non-Goals For This Milestone

The current chrome does not execute untrusted JavaScript, render arbitrary remote HTML, or embed a system webview. That is deliberate while the core policy and tab lifecycle are being built.
