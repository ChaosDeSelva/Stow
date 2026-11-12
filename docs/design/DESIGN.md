# Stow design system

Palette, type, shape, and component tokens for Stow's UI. Light mode is the base; dark mode is derived (see bottom).

## Colors
primary: #f7a501 (yellow-orange CTA) · primary-pressed: #dd9001 · primary-active: #b17816 · on-primary: #23251d
ink: #23251d (headlines, button text) · body: #4d4f46 (paragraph) · charcoal: #33342d · mute: #6c6e63 (metadata) · ash: #9b9c92 (disabled text) · stone: #b6b7af
hairline: #bfc1b7 (1px card border, rules) · hairline-soft: #dcdfd2 (in-card row divider) · on-dark: #ffffff
canvas: #eeefe9 (page background, nav, sidebar, footer) · surface-soft: #e5e7e0 (secondary button, sub-nav, inline code) · surface-card: #ffffff · surface-doc: #fcfcfa · surface-dark: #23251d (code blocks)
link-blue: #1d4ed8 · link-teal: #1078a3 (doc inline links) · accent-blue: #2c84e0 / soft #dceaf6 · accent-red: #cd4239 / soft #f7d6d3 · accent-green: #2c8c66 / soft #d9eddf · accent-purple: #7c44a6 / soft #e7d8ee · focus-ring: rgba(59,130,246,0.5)

## Typography
Family: "IBM Plex Sans Variable", "IBM Plex Sans", -apple-system, system-ui, sans-serif. Weights 400/500/600/700/800. Mono: ui-monospace / Source Code Pro (JetBrains Mono ok).
display-xl 36/700/1.5 · display-lg 24/800/1.33/-0.6px · heading-lg 21/700/1.4/-0.5px · heading-md 20/700/1.4 · heading-sm 18/700/1.5 UPPERCASE · heading-sm-mixed 18/600/1.56
body-md 16/400/1.5 · body-strong 16/600 · body-sm 15/400/1.71 · body-sm-strong 15/600 · body-xs 14/500/1.43 (sidebar items, metadata) · caption-md 14/700/1.71 · caption-sm 13/500/1.5 · caption-xs 12/600/1.33 UPPERCASE (badges) · utility-xs 12/700/1.33 UPPERCASE (eyebrows, section headers)
button-md 14/700/1.5 · button-sm 13/500/1 · code-sm 14/400/1.43 · code-xs 14/500/1.43
Hierarchy comes from weight contrast more than size. No italics, no decorative faces.

## Shape, spacing, elevation
rounded: xs 2px (inline code) · sm 4px (inputs, small buttons, chips) · md 6px (cards, buttons, code blocks — the default) · lg 8px · full 9999px (pill chips, pill CTA)
spacing: 2 / 4 / 8 / 12 / 16 / 24 / 32 · section 80
Elevation: NO drop shadows anywhere. Level 1 = 1px solid hairline; level 2 = 1px hairline-soft row dividers; the only "elevated" surface is the dark code block (color, not shadow).

## Components
button-primary: bg primary, text on-primary (dark olive), button-md, radius md, padding 8px 16px, height 40px; pressed → primary-pressed.
button-secondary: bg surface-soft, text ink, same metrics. button-tertiary: transparent, text ink, padding 8px 12px. button-disabled: bg surface-soft, text ash.
text-input / search-input: bg surface-card, text ink, 1px hairline, body-md, padding 8px 12px, height 36px, radius md; focused → 2px accent-blue border + focus-ring outline.
product-card / feature-tile / doc-card: bg surface-card (doc: surface-doc), 1px hairline, radius md, padding 24px (tile 20px).
product-tab: transparent, text body, body-strong, padding 8px 12px, radius md; active → bg surface-card (white lifts off cream), text ink.
pill-tab: transparent, text body, button-sm, padding 6px 14px, radius full; active → bg ink, text on-dark (fully inverted).
badge-uppercase: transparent, text body, utility-xs. badge-promo: bg accent-blue-soft, text link-blue, caption-xs, padding 2px 8px, radius full.
banner-tip-{blue,green,red,purple}: soft accent bg, text ink, padding 16px 20px, radius md, leading Font Awesome icon in the strong accent of the tint (circle-info, circle-check, triangle-exclamation, key).
code-block: bg surface-dark, text on-dark, code-sm, padding 16px 20px, radius md. inline-code: bg surface-soft, text ink, code-xs, padding 2px 6px, radius xs.
primary-nav: bg canvas, text ink, body-strong, height 56px. sub-nav-strip: bg surface-soft, text body, body-xs, height 40px. doc-sidebar: bg canvas, text body, body-xs, width 240px, rounded outline-icon section headers, nested links indented. footer: canvas, 1px hairline top rule.
link-inline: link-teal, no underline, underline on focus.

## Do / Don't
Do: cream canvas (never pure white page) · yellow-orange reserved for THE primary action, at most one per view · IBM Plex Sans everywhere · 1px hairline borders · 24px card padding.
Don't: drop shadows on cards · a second saturated CTA color · pure white or dark hero canvases · pastel callouts as card backgrounds · uppercase outside eyebrows/badges.

## Dark mode (derived)
canvas #1d1f1a · surface-soft #2a2c25 · surface-card #23251d · surface-doc #262821 · hairline #3f4138 · hairline-soft #33352e · ink #eeefe9 · body #c9cbc0 · mute #9b9c92 · ash #6c6e63 · primary stays #f7a501 with on-primary #23251d · link-teal #4fb3dd · accent softs darkened (blue #1f3242, green #1f3a2d, red #452320, purple #35253f).
