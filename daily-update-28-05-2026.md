# Fortrust Daily Update — 28 May 2026

## Changes Made

### Keyboard Shortcuts
- Added Ctrl+T to open new tab
- Added Ctrl+W to close current tab
- Added Ctrl+L to focus and clear omnibox
- Added Ctrl+R to reload current page
- Added Esc to close sidebar or blur address bar

### Speed Dial Hover Animation
- Fixed hover_scale not being applied to tile rendering — tiles now scale up on hover

### Tab Bar
- Added hover background overlay on non-active tabs
- Active tab now displays surface_deepest background

### Sidebar Shadow
- Replaced flat 4px shadow with 8-step gradient fade on overlay right edge

### Add Site Dialog
- Added URL/title input modal triggered by "+" button in search bar and "Add site" tile
- Cancel and Add buttons, Enter to confirm, Esc to dismiss

### Housekeeping
- Renamed W/H variables to w/h to fix snake_case warnings
- Added #[allow(dead_code)] to 3 unused methods

## Pending
- Scrollable sidebar content
