// Shared Tailwind config and helpers for the Teams and API key mockups.
// Tokens are copied from crates/admin-ui/web/src/styles/globals.css (dark theme).
tailwind.config = {
  theme: {
    extend: {
      colors: {
        background: '#07111c',
        card: '#0c1a29',
        popover: '#0e2133',
        secondary: '#102336',
        muted: '#0c1f31',
        accent: '#0d2841',
        'surface-muted': '#0c1e2f',
        foreground: '#e8edf3',
        'muted-foreground': '#94a7bb',
        primary: '#bb9af7',
        'primary-foreground': '#040c15',
        border: 'rgb(236 240 247 / 0.08)',
        input: 'rgb(236 240 247 / 0.12)',
        destructive: '#fb7185',
        success: '#73daca',
        warning: '#eaa518',
        sidebar: '#040c15',
      },
      fontFamily: {
        sans: ['Geist', 'ui-sans-serif', 'sans-serif'],
        mono: ['"Geist Mono"', 'ui-monospace', 'monospace'],
      },
    },
  },
}

const AVATAR_COLORS = ['#B0E0E0', '#106090', '#2080B0', '#7C3AED', '#22C55E']

function hash(text) {
  let value = 0
  for (const char of text) value = (value * 31 + char.charCodeAt(0)) >>> 0
  return value
}

/** A stand-in for GeneratedAvatar: a two-stop gradient picked from the Oceans avatar palette. */
function avatar(name, size = 28, ring = false) {
  const h = hash(name)
  const first = h % 5
  const a = AVATAR_COLORS[first]
  // Always a different second stop, so no avatar collapses to one dark blue disc.
  const b = AVATAR_COLORS[(first + 1 + ((h >> 3) % 4)) % 5]
  const angle = h % 360
  return `<span class="inline-block shrink-0 rounded-full ${ring ? 'ring-2 ring-card' : ''}" style="width:${size}px;height:${size}px;background:linear-gradient(${angle}deg,${a},${b})" aria-hidden="true"></span>`
}

const ICONS = {
  chevronRight:
    '<svg viewBox="0 0 24 24" class="size-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m9 6 6 6-6 6"/></svg>',
  chevronDown:
    '<svg viewBox="0 0 24 24" class="size-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>',
  more: '<svg viewBox="0 0 24 24" class="size-4" fill="currentColor"><circle cx="5" cy="12" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="19" cy="12" r="1.5"/></svg>',
  search:
    '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>',
  plus: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
  x: '<svg viewBox="0 0 24 24" class="size-3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>',
  tag: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M3 12V4h8l9 9-8 8z"/><circle cx="7.5" cy="8.5" r="1.25"/></svg>',
  user: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="12" cy="8" r="3.5"/><path d="M5 20a7 7 0 0 1 14 0"/></svg>',
  bot: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="8" width="16" height="11" rx="3"/><path d="M12 8V5M9 13h.01M15 13h.01"/></svg>',
  grid: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="4" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5"/><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5"/><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5"/></svg>',
  list: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="m4 7 1.5 1.5L8 6M4 13l1.5 1.5L8 12M11 7h9M11 13h9M11 19h9"/></svg>',
  shield:
    '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"><path d="M12 3 5 6v5c0 4.5 3 8 7 10 4-2 7-5.5 7-10V6z"/></svg>',
  sliders:
    '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/></svg>',
  server:
    '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="4" y="4" width="16" height="7" rx="2"/><rect x="4" y="13" width="16" height="7" rx="2"/><path d="M8 7.5h.01M8 16.5h.01" stroke-linecap="round"/></svg>',
  eye: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M2.5 12S6 5 12 5s9.5 7 9.5 7-3.5 7-9.5 7-9.5-7-9.5-7z"/><circle cx="12" cy="12" r="2.5"/></svg>',
  copy: '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></svg>',
  unfold:
    '<svg viewBox="0 0 24 24" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m7 9 5-5 5 5M7 15l5 5 5-5"/></svg>',
}
